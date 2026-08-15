This describes the fish history architecture, and upcoming JSON Lines-based fish history file format.

## History files

The fish history file stores commands the user enters, along with associated metadata. History can be displayed and queried by the user, and supports autosuggestions, offering to complete the user's command line with previously run commands.

By default, all sessions append items to a single file. Users may create separate "sessions" with the `fish_history` variable.

fish appends to the history file (via `O_APPEND`) after each interactive command is run. On local filesystems, fish uses an advisory lock to serialize writers. On remote filesystems, fish relies on the atomicity of `O_APPEND` for small writes. If even that fails, an individual history item may be corrupted, but the remainder of the file remains valid.

This file grows until it is "vacuumed," which means discarding items (according to some criteria, such as staleness) to reduce the item count below a limit. Item deletion is implemented via marking an item as discardable, and then vacuuming.

### File parsing

Parsing the entire history file at startup would slow down shell launch and consume unnecessary memory. Instead, fish quickly scans the history file and records only the offsets of the record delimiters. These offsets form an index into the history file, allowing fish to locate individual records quickly, and parse them lazily.

### YAML-like file format (historical)

Starting with fish 2.0.0, the fish history file format is "YAML-like" meaning it is superficially similar to YAML but differs from it in key ways (such as failing to escape colons) which makes it invalid to parse with a standard YAML parser. This is a historical implementation mistake that has been preserved.

In this format, very little metadata was recorded. Items were deduplicated without concern for metadata. In particular, there is no easy way to extend an item with data that arrives after the command has finished, such as its exit status and duration.

## JSONL file format

In an upcoming version of fish, the file format will be switched to [JSON Lines](https://jsonlines.org), also known as NDJSON ("newline-delimited JSON"). The encoding will remain UTF-8, with fish's normal use of the PUA for non-encodeable bytes. Record boundaries are simply newlines.

Each line contains a separate record. Thus, should any record become corrupted (e.g. torn writes on NFS), the file parser simply advances to the next newline and continue from there. Corrupted records are deleted on vacuuming.

Note `jq` supports JSON Lines.

The following describes the planned JSON Lines format.

### History item IDs

A fish history item is identified by its "id:" a 64 bit unsigned integer, which contains a 48 bit _timestamp_ and a 16 bit _nonce_.

```
  Bits 63                                                  16 15             0
  +----------------------------------------------------------+---------------+
  |                     timestamp (48 bits)                  |  nonce (16 b) |
  +----------------------------------------------------------+---------------+
```

The timestamp field records milliseconds since the epoch. This provides millisecond precision over nearly 9000 years.

The nonce is randomized per millisecond, and incremented within a millisecond. This ensures that item IDs from a single session are monotone increasing, and collisions between sessions are very unlikely: you'd need over one hundred items added by different sessions within a millisecond to have a 10% chance of a collision.

(This design is similar to a [ULID](https://github.com/ulid/spec), except reduced from 128 to 64 bits for speed and file size considerations.)

IDs are stored in the file as base64, such as `"AZ9StuUE8AY"`. This reduces file size compared to a decimal expansion. Note that sorting by numeric id also orders items chronologically.

### History item records

A history item is represented by a collection of records, each sharing the same item ID. The first record contains the command itself; records later in the history file may annotate the item with additional metadata, such as duration.

Note records may be physically interleaved in the file:

```
  ┌─────────────────────────────────────────────────────────┐
  │  [ID=0x1a2b]  cmd: "git pull"          ← initial record │
  │  [ID=0x1a2b]  cwd: "/home/fish/src"    ← metadata       │
  │  [ID=0x1a2c]  cmd: "make test"         ← another item   │
  │  [ID=0x1a2b]  exit: 0                  ← more metadata  │
  │  [ID=0x1a2b]  dur: 532                 ← more metadata  │
  └─────────────────────────────────────────────────────────┘
```

### Initial keys

The planned keys in the history file format are:

- `id` - the history item ID
- `cmd` - the command text
- `cwd` - the current working directory at command execution, with `$HOME` replaced with `~`
- `exit` - the command’s exit code
- `dur` - execution duration in milliseconds
- `files` - the list of arguments that resolved to files, used for autosuggestion hinting
- `sid` - A session identifier unique to each fish instance, used to attach commands to each session

Additional metadata keys may be introduced in future versions.

### Vacuum policy

fish will at times _vacuum_ the JSONL file to reduce its size. Vacuuming involves:

1. Collapsing multiple records for the same ID into a single record
2. Removing oldest items to enforce the item limit
3. Skipping deleted items

fish performs vacuuming by writing an adjacent file and atomically moving it into place. This is performed whenever:

1. The number of items in history reaches the _vacuum threshold_, currently 640K items, or
2. The size of new (unvacuumed) data reaches 25% of the file and at least 64 KiB, or
3. An item is deleted from history

Vacuuming keeps the 512K newest items. No deduplication is performed. 512K compacted history items are expected to occupy roughly 64-128 MiB in practice.
