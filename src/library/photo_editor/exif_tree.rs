//! EXIF as TIFF stores it, read from any TIFF-structured block (a JPEG's EXIF, a HEIC's, a
//! RAW file itself) and written back as one compact block in the same byte order, so every
//! value, maker notes included, keeps its exact bytes.

/// Byte order of a TIFF block; values are kept in it, so nothing needs converting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Order {
    Little,
    Big,
}

impl Order {
    fn u16(self, b: &[u8]) -> u16 {
        let b = [b[0], b[1]];
        match self {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        }
    }

    fn u32(self, b: &[u8]) -> u32 {
        let b = [b[0], b[1], b[2], b[3]];
        match self {
            Order::Little => u32::from_le_bytes(b),
            Order::Big => u32::from_be_bytes(b),
        }
    }

    fn put16(self, v: u16) -> [u8; 2] {
        match self {
            Order::Little => v.to_le_bytes(),
            Order::Big => v.to_be_bytes(),
        }
    }

    fn put32(self, v: u32) -> [u8; 4] {
        match self {
            Order::Little => v.to_le_bytes(),
            Order::Big => v.to_be_bytes(),
        }
    }
}

/// One tag: its TIFF type, count, and value bytes in the block's byte order.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Field {
    pub tag: u16,
    pub kind: u16,
    pub count: u32,
    pub value: Vec<u8>,
    /// Where an out-of-line value sat in the block it was read from.
    source: Option<u32>,
}

/// The directories EXIF uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dir {
    Ifd0 = 0,
    Exif = 1,
    Gps = 2,
    Interop = 3,
}

const EXIF_POINTER: u16 = 0x8769;
const GPS_POINTER: u16 = 0x8825;
const INTEROP_POINTER: u16 = 0xA005;
const MAKER_NOTE: u16 = 0x927C;
/// The most entries one directory may claim; more means the block is damaged.
const MAX_ENTRIES: usize = 4096;

/// Bytes per value of each TIFF type, indexed by type; 0 for types TIFF doesn't define.
const TYPE_SIZES: [usize; 19] = [0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8, 4, 0, 0, 8, 8, 8];

fn value_size(kind: u16, count: u32) -> Option<usize> {
    let size = *TYPE_SIZES.get(usize::from(kind))?;
    (size > 0).then(|| size.saturating_mul(count as usize))
}

/// IFD0, Exif, GPS, and interoperability directories, and the maker note.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ExifTree {
    pub order: Order,
    dirs: [Vec<Field>; 4],
    /// The maker note's bytes and their offset in the source block: makers whose notes point
    /// into the block from its start can only be read where the note was.
    maker_note: Option<(Vec<u8>, u32)>,
}

impl ExifTree {
    pub(super) fn empty(order: Order) -> Self {
        Self {
            order,
            dirs: Default::default(),
            maker_note: None,
        }
    }

    /// Reads a TIFF block from its header; `None` when it isn't one.
    pub(super) fn parse(tiff: &[u8]) -> Option<Self> {
        let order = match tiff.get(..2)? {
            b"II" => Order::Little,
            b"MM" => Order::Big,
            _ => return None,
        };
        let ifd0 = order.u32(tiff.get(4..8)?) as usize;
        let mut tree = Self::empty(order);
        tree.dirs[0] = read_dir(tiff, order, ifd0)?;
        tree.follow(tiff, Dir::Ifd0, EXIF_POINTER, Dir::Exif);
        tree.follow(tiff, Dir::Ifd0, GPS_POINTER, Dir::Gps);
        tree.follow(tiff, Dir::Exif, INTEROP_POINTER, Dir::Interop);
        if let Some(note) = tree.take(Dir::Exif, MAKER_NOTE) {
            tree.maker_note = Some((note.value, note.source.unwrap_or(0)));
        }
        Some(tree)
    }

    /// Replaces the pointer `tag` in `from` with the directory it points to.
    fn follow(&mut self, tiff: &[u8], from: Dir, tag: u16, to: Dir) {
        let Some(pointer) = self.take(from, tag) else {
            return;
        };
        if let Some(at) = pointer.value.get(..4).map(|b| self.order.u32(b) as usize) {
            self.dirs[to as usize] = read_dir(tiff, self.order, at).unwrap_or_default();
        }
    }

    /// Takes `other`'s IFD0 as this tree's `dir`, when both share a byte order.
    pub(super) fn adopt(&mut self, dir: Dir, other: Self) {
        if other.order == self.order {
            let [ifd0, ..] = other.dirs;
            self.dirs[dir as usize] = ifd0;
        }
    }

    pub(super) fn get(&self, dir: Dir, tag: u16) -> Option<&Field> {
        self.dirs[dir as usize].iter().find(|f| f.tag == tag)
    }

    pub(super) fn take(&mut self, dir: Dir, tag: u16) -> Option<Field> {
        let fields = &mut self.dirs[dir as usize];
        let index = fields.iter().position(|f| f.tag == tag)?;
        Some(fields.remove(index))
    }

    /// Adds `field`, or replaces the one with its tag; tags stay in ascending order.
    pub(super) fn set(&mut self, dir: Dir, field: Field) {
        let fields = &mut self.dirs[dir as usize];
        match fields.binary_search_by_key(&field.tag, |f| f.tag) {
            Ok(index) => fields[index] = field,
            Err(index) => fields.insert(index, field),
        }
    }

    pub(super) fn retain(&mut self, dir: Dir, keep: impl Fn(u16) -> bool) {
        self.dirs[dir as usize].retain(|f| keep(f.tag));
    }

    #[cfg(test)]
    pub(super) fn has_maker_note(&self) -> bool {
        self.maker_note.is_some()
    }

    #[cfg(test)]
    pub(super) fn set_maker_note(&mut self, bytes: &[u8], at: u32) {
        self.maker_note = Some((bytes.to_vec(), at));
    }

    pub(super) fn short(&self, tag: u16, v: u16) -> Field {
        field(tag, 3, 1, self.order.put16(v).to_vec())
    }

    pub(super) fn long(&self, tag: u16, v: u32) -> Field {
        field(tag, 4, 1, self.order.put32(v).to_vec())
    }

    pub(super) fn rationals(&self, tag: u16, parts: &[(u32, u32)]) -> Field {
        let bytes = parts
            .iter()
            .flat_map(|(n, d)| self.order.put32(*n).into_iter().chain(self.order.put32(*d)))
            .collect();
        field(tag, 5, parts.len() as u32, bytes)
    }

    pub(super) fn ascii(tag: u16, text: &str) -> Field {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        field(tag, 2, bytes.len() as u32, bytes)
    }

    pub(super) fn bytes(tag: u16, kind: u16, data: Vec<u8>) -> Field {
        field(tag, kind, data.len() as u32, data)
    }

    /// The block, at most `limit` bytes: with the maker note when it fits, else without it.
    pub(super) fn write(&self, limit: usize) -> Option<Vec<u8>> {
        let with_note = self.write_with(self.maker_note.as_ref());
        if with_note.len() <= limit {
            return Some(with_note);
        }
        Some(self.write_with(None)).filter(|block| block.len() <= limit)
    }

    /// IFD0, then Exif, GPS, and interoperability, each followed by its long values; the
    /// maker note last, at its old offset when everything before it leaves room.
    fn write_with(&self, note: Option<&(Vec<u8>, u32)>) -> Vec<u8> {
        let has = |dir: Dir| !self.dirs[dir as usize].is_empty();
        let (ifd0, exif) = self.with_pointers(note);
        let exif_used = !exif.is_empty();
        let mut out = match self.order {
            Order::Little => b"II".to_vec(),
            Order::Big => b"MM".to_vec(),
        };
        out.extend(self.order.put16(42));
        out.extend(self.order.put32(8));
        let ifd0_slots = self.write_dir(&mut out, &ifd0);
        let exif_slots = if exif_used {
            self.place(&mut out, &ifd0_slots, EXIF_POINTER);
            self.write_dir(&mut out, &exif)
        } else {
            Vec::new()
        };
        if has(Dir::Gps) {
            self.place(&mut out, &ifd0_slots, GPS_POINTER);
            self.write_dir(&mut out, &self.dirs[Dir::Gps as usize]);
        }
        if has(Dir::Interop) {
            self.place(&mut out, &exif_slots, INTEROP_POINTER);
            self.write_dir(&mut out, &self.dirs[Dir::Interop as usize]);
        }
        if let Some((bytes, at)) = note {
            let at = *at as usize;
            if out.len() <= at {
                out.resize(at, 0);
                self.patch(&mut out, &exif_slots, MAKER_NOTE, at);
            } else {
                self.place(&mut out, &exif_slots, MAKER_NOTE);
            }
            out.extend(bytes);
        }
        out
    }

    /// IFD0 and Exif with a placeholder for each directory and the maker note that follows
    /// them; Exif comes back empty when nothing needs it.
    fn with_pointers(&self, note: Option<&(Vec<u8>, u32)>) -> (Vec<Field>, Vec<Field>) {
        let has = |dir: Dir| !self.dirs[dir as usize].is_empty();
        let mut ifd0 = self.dirs[0].clone();
        let mut exif = self.dirs[1].clone();
        ifd0.retain(|f| !matches!(f.tag, EXIF_POINTER | GPS_POINTER));
        exif.retain(|f| !matches!(f.tag, INTEROP_POINTER | MAKER_NOTE));
        if has(Dir::Gps) {
            insert(&mut ifd0, self.long(GPS_POINTER, 0));
        }
        if has(Dir::Interop) {
            insert(&mut exif, self.long(INTEROP_POINTER, 0));
        }
        if let Some((bytes, _)) = note {
            let placeholder = field(MAKER_NOTE, 7, bytes.len() as u32, Vec::new());
            insert(&mut exif, placeholder);
        }
        if !exif.is_empty() {
            insert(&mut ifd0, self.long(EXIF_POINTER, 0));
        }
        (ifd0, exif)
    }

    /// Points the slot for `tag` at the end of `out`, evened up, where its data goes next.
    fn place(&self, out: &mut Vec<u8>, slots: &[(u16, usize)], tag: u16) {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let at = out.len();
        self.patch(out, slots, tag, at);
    }

    fn patch(&self, out: &mut [u8], slots: &[(u16, usize)], tag: u16, at: usize) {
        if let Some((_, slot)) = slots.iter().find(|(t, _)| *t == tag) {
            out[*slot..*slot + 4].copy_from_slice(&self.order.put32(at as u32));
        }
    }

    /// Writes one directory and its long values; returns where each pointer slot is, for the
    /// pointers and the maker note, whose targets come later.
    fn write_dir(&self, out: &mut Vec<u8>, fields: &[Field]) -> Vec<(u16, usize)> {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let start = out.len();
        let data_start = start + 2 + 12 * fields.len() + 4;
        let mut data: Vec<u8> = Vec::new();
        let mut slots = Vec::new();
        out.extend(self.order.put16(fields.len() as u16));
        for f in fields {
            out.extend(self.order.put16(f.tag));
            out.extend(self.order.put16(f.kind));
            out.extend(self.order.put32(f.count));
            let deferred = matches!(
                f.tag,
                EXIF_POINTER | GPS_POINTER | INTEROP_POINTER | MAKER_NOTE
            );
            if deferred {
                slots.push((f.tag, out.len()));
                out.extend([0; 4]);
            } else if f.value.len() <= 4 {
                let mut inline = f.value.clone();
                inline.resize(4, 0);
                out.extend(inline);
            } else {
                out.extend(self.order.put32((data_start + data.len()) as u32));
                data.extend(&f.value);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
            }
        }
        out.extend([0; 4]);
        out.extend(data);
        slots
    }
}

fn field(tag: u16, kind: u16, count: u32, value: Vec<u8>) -> Field {
    Field {
        tag,
        kind,
        count,
        value,
        source: None,
    }
}

fn insert(fields: &mut Vec<Field>, field: Field) {
    let index = fields.partition_point(|f| f.tag < field.tag);
    fields.insert(index, field);
}

/// The directory at `at`, skipping entries whose values lie outside the block.
fn read_dir(tiff: &[u8], order: Order, at: usize) -> Option<Vec<Field>> {
    let count = usize::from(order.u16(tiff.get(at..at + 2)?));
    if count > MAX_ENTRIES {
        return None;
    }
    let mut fields = Vec::with_capacity(count);
    for i in 0..count {
        let start = at + 2 + 12 * i;
        let Some(entry) = tiff.get(start..start + 12) else {
            break;
        };
        if let Some(field) = read_entry(tiff, order, entry) {
            fields.push(field);
        }
    }
    fields.sort_by_key(|f| f.tag);
    Some(fields)
}

fn read_entry(tiff: &[u8], order: Order, entry: &[u8]) -> Option<Field> {
    let (tag, kind, count) = (
        order.u16(&entry[0..2]),
        order.u16(&entry[2..4]),
        order.u32(&entry[4..8]),
    );
    let size = value_size(kind, count)?;
    if size <= 4 {
        return Some(field(tag, kind, count, entry[8..8 + size].to_vec()));
    }
    let offset = order.u32(&entry[8..12]);
    let value = tiff.get(offset as usize..(offset as usize).checked_add(size)?)?;
    Some(Field {
        source: Some(offset),
        ..field(tag, kind, count, value.to_vec())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A big-endian block: IFD0 (Make, Orientation 6, a strip), Exif (a date, a maker note
    /// that sits at offset 200), and GPS.
    fn sample() -> Vec<u8> {
        let mut tree = ExifTree::empty(Order::Big);
        tree.set(Dir::Ifd0, ExifTree::ascii(0x010F, "Canon"));
        tree.set(Dir::Ifd0, tree.short(0x0112, 6));
        tree.set(Dir::Ifd0, tree.long(0x0111, 123_456));
        tree.set(Dir::Exif, ExifTree::ascii(0x9003, "2024:01:15 19:55:15"));
        tree.set(Dir::Gps, ExifTree::ascii(0x0001, "N"));
        tree.maker_note = Some((b"NOTE-BYTES-0123456789".to_vec(), 200));
        tree.write(usize::MAX).unwrap()
    }

    #[test]
    fn blocks_read_back_the_same_in_either_order() {
        let block = sample();
        assert!(block.starts_with(b"MM\0*"));
        let tree = ExifTree::parse(&block).unwrap();
        assert_eq!(tree.order, Order::Big);
        assert_eq!(tree.get(Dir::Ifd0, 0x010F).unwrap().value, b"Canon\0");
        assert_eq!(tree.get(Dir::Ifd0, 0x0112).unwrap().value, [0, 6]);
        assert_eq!(tree.get(Dir::Exif, 0x9003).unwrap().count, 20);
        assert_eq!(tree.get(Dir::Gps, 0x0001).unwrap().value, b"N\0");
        assert_eq!(
            tree.maker_note,
            Some((b"NOTE-BYTES-0123456789".to_vec(), 200))
        );
    }

    #[test]
    fn rewriting_keeps_the_maker_note_where_it_was() {
        let tree = ExifTree::parse(&sample()).unwrap();
        let again = ExifTree::parse(&tree.write(usize::MAX).unwrap()).unwrap();
        assert_eq!(again, tree);
    }

    #[test]
    fn maker_notes_too_big_for_the_limit_are_left_out() {
        let tree = ExifTree::parse(&sample()).unwrap();
        let small = tree.write(150).unwrap();
        let read = ExifTree::parse(&small).unwrap();
        assert!(!read.has_maker_note());
        assert_eq!(read.get(Dir::Ifd0, 0x010F).unwrap().value, b"Canon\0");
        assert!(tree.write(20).is_none());
    }

    #[test]
    fn damaged_blocks_are_refused_or_skipped() {
        assert!(ExifTree::parse(b"not a tiff").is_none());
        let mut block = sample();
        block.truncate(40);
        let tree = ExifTree::parse(&block);
        assert!(tree.is_none_or(|t| t.get(Dir::Exif, 0x9003).is_none()));
    }
}
