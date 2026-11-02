pub fn capped(width: i32, height: i32) -> (i32, i32) {
    match std::env::var("MIMICK_WINDOW_SIZE")
        .ok()
        .as_deref()
        .and_then(parse)
    {
        Some((max_width, max_height)) => (width.min(max_width), height.min(max_height)),
        None => (width, height),
    }
}

fn parse(value: &str) -> Option<(i32, i32)> {
    let (width, height) = value.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_width_by_height() {
        assert_eq!(parse("1000x700"), Some((1000, 700)));
    }

    #[test]
    fn rejects_malformed_sizes() {
        assert_eq!(parse("1000"), None);
        assert_eq!(parse("widex700"), None);
        assert_eq!(parse(""), None);
    }
}
