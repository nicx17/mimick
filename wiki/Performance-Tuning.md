# Performance Tuning

Default settings work for most setups. The options below are useful when you have a large library, limited hardware, or want to control how aggressively Mimick uses bandwidth and memory.

---

## Parallel Uploads

**Setting:** Settings → Uploads → Parallel Uploads (1–10, default 3)

Each worker is an independent async upload task. Workers share the same HTTP connection pool, so adding more workers increases concurrent uploads rather than opening separate connections.

| Scenario | Suggested value |
| :--- | :--- |
| Slow or metered connection | 1–2 |
| Home LAN, typical library | 3 (default) |
| Fast LAN or local server, large backlog | 5–8 |
| Saturating uploads cause other issues | Reduce back toward default |

Workers apply to the background sync engine only. Album sync (triggered from the library view) runs as a separate task and is not affected by this setting.

Changing the worker count takes effect immediately — no restart needed.

---

## When Mimick Starts, Check

**Setting:** Settings → Uploads → When Mimick Starts, Check

Controls how much work Mimick does each time it launches. Each folder can choose its own in its Folder Rules.

| Mode | What it does | When to use it |
| :--- | :--- | :--- |
| **All Files** | Hashes every file in every watch folder | Initial setup; after adding many new folders |
| **Changed in the Last 7 Days** | Hashes files modified in the last 7 days | Ongoing use with occasional gaps |
| **New Files Only** | Only processes files not already in the sync index | Daily driver once the library is fully synced |

Switch from All Files to New Files Only once your initial sync is complete. Startup time on a large library (tens of thousands of files) drops significantly.

---

## Thumbnail Memory Cache

There is no setting for this: Mimick sizes the cache from your RAM.

The library view keeps decoded thumbnails and lightbox previews in memory so scrolling back is instant. The cache can grow to 20% of your RAM, but never below 500 MB or above 3 GB. When it's full, the images you looked at longest ago are dropped first. Images also stay in the on-disk cache (**Cache Limit (MB)** in Settings → Storage), so a dropped image reloads from disk instead of the server.

When you close the library window, the cache is emptied and the memory is given back to the system, so Mimick running in the tray uses little RAM.

---

## Open Originals in Lightbox

**Setting:** Settings → Library → Open Originals in Lightbox (switch, default off)

When off, the lightbox loads a ~1440px server-generated preview image. When on, it loads the original file.

- Full-resolution mode uses significantly more bandwidth and takes longer to load on slow connections.
- Use it only if you need to inspect originals or are on a fast LAN connection to your Immich server.

---

## Large Libraries: Recommended Configuration

For libraries with more than ~50,000 files:

1. Set **When Mimick Starts, Check** to **New Files Only** after the initial sync.
2. Keep **Parallel Uploads** at 3–5 unless uploads are a bottleneck.
3. Close the library window when you're done browsing; that frees the memory its thumbnail cache used.
4. Leave **Open Originals in Lightbox** off unless needed.
