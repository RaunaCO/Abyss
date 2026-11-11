# Local files

Settings > Local files owns file/folder imports, watched folders, the imported
list, removal from the index, per-file metadata editing and restoration. The
top import button links there. The sidebar opens the unified Library, with
local tracks, albums and artists derived from artist and album-artist tags.
They share category sorting, filters, search, detail pages and context menus.
Local content works without signing in, including the Library button on login.
There is no separate local-files page. Stopping folder watching or
removing an import never deletes music. Library file/album menus also link to Settings, where complete albums can
be removed from the index.

Title, artist, album artist, album, genre, year, track number, disc number and
front artwork are written into MP3, FLAC and M4A/MP4 tags using Lofty 0.25.5.
WAV edits are stored only in local-library.json. Blank optional values remove
those fields. Covers accept PNG/JPEG up to 16 MiB. Titles are required and
numeric fields accept nonnegative integers.

For tag writes and restoration, Abyss first checks the read-only attribute and
opens the source with exclusive read/write access on Windows. Files open in
another application or playing in Abyss are reported without writing. A sibling
temporary receives the full copy and tag changes; it is flushed, parsed again
and inspected before an atomic replacement. A failed write leaves the source
intact. Application temporary files are reclaimed, user files are never deleted.

Before the first replacement, local-originals/<path-hash>/audio stores the entire
original file and entry.json stores its original index entry. This costs one
extra file's worth of disk space and preserves tags the editor does not expose.
The backup is never overwritten. Restore original copies it through the same
temporary-file path; it does not rewrite the backup. WAV keeps only the index
entry. Do not manually repurpose the same imported path for unrelated music
while expecting its first backup to change.

Successful edits rebuild the local album index, queue metadata and artwork
references. Artwork filenames include a content hash so changed covers cannot
reuse a stale image-cache key. Old artwork is retained. If the audio replacement
succeeds but index persistence fails, a separate warning asks for another save;
the refreshed metadata remains in memory. Audio and index are separate files
and cannot form a single filesystem transaction.

Lofty sources and licences are pinned under vendor/lofty-rs. See
[the vendor note](../vendor/lofty-rs/ABYSS-VENDOR.md). No audio-engine code is
changed by the editor. Real file round trips, lock/read-only cases and visual
review in all six languages remain owner validation; no tests were run.
