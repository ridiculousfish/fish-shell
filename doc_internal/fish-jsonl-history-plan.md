This describes the proposed fish history file format based on JSON Lines.

## History files

The fish history file stores commands the user enters, along with associated metadata. History can be displayed and queried by the user. History also supports autosuggestions, offering to complete the command line with previously run commands.

### Legacy file format

The fish history file format, prior to this proposal, is a pseudo-YAML file. Commands are appended to it (with `O_APPEND`) after they are run. This had some disadvantages:

1. It's difficult to add additional fields to history items
2. It's difficult to annotate an item after it has been posted to history (e.g. with duration, exit status, etc).

### New format requirements

Any new fish history file format has a number of requirements, some original and some new.

### Original requirements

- _Shared file_. Multiple sessions collaborate on a shared history. fish doesn't overwrite the whole thing like bash does.
- _Private view_. A fish session loads history that exists at startup. Commands from other running sessions aren’t visible unless the user explicitly requests it (`history merge`).
- _Fast and expansive_ - History files can be very large. fish must avoid a big up-front parse at startup.
- _Network safe_ History files may be stored on shared network filesystems that lack reliable locking.
  - Concurrent writes must not corrupt the file; losing an entry in rare race conditions is tolerable.
- _Serverless_ There is no "history file server" that coordinates fish sessions; sessions must coordinate themselves. (This is derived from the old universal variables daemon; upgrading this was very painful).

### New requirements

Any new file format should at least offer this:

- _Millisecond precision_ fish history timestamps have millisecond precision over 100+ years.
- _Metadata support_ fish history items my be annotated with additional metadata (`$CWD`, session ID, file path detection, etc)
  - This metadata may be produced after the command has executed (duration, exit status)
  - Future versions of fish can introduce new metadata fields without breaking compatibility.

### Nice to haves

- _Text format_ Text is the preferred Unix file format. Users can inspect and manipulate the history file with standard Unix tools.

## Design

These requirements push us towards an append-only database.

### History item IDs

A fish history item is identified by a 64 bit number, which contains a 48 bit _timestamp_ and a 16 bit _nonce_.

```
  Bits 63                                                          16 15          0
  +----------------------------------------------------------+---------------+
  |                     timestamp (48 bits)                  |  nonce (16 b) |
  +----------------------------------------------------------+---------------+
            most significant bits (MSB)                            least significant bits (LSB)
```

The timestamp is the number of milliseconds since the epoch, allowing for millions of years of range.

The nonce is chosen randomly at session startup and incremented with each command, making collisions between concurrent sessions very unlikely.

Note that sorting by ID also orders items chronologically.

### History item records

A history item is represented by a collection of records, each sharing the same item ID. The _initial_ record contains the command itself; records later in the history file may annotate the item with one or more items of metadata.

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

### File format

The history file is in [JSON Lines](https://jsonlines.org/) format with UTF-8. Each line is a JSON object and represents a single record.

Each record contains at least the "id" key, which is the history item ID as described above. It also contains some (ideally short) keys populating the history item.

A single record may have multiple pieces of data. If multiple records have the same metadata for the same history item, the last one wins.

Each line contains a separate record. Thus, should any record become corrupted (e.g. torn writes on NFS), the file parser can simply advance to the next newline and continue from there.

Note `jq` supports JSON Lines.

### Initial keys

The standard keys currently used in the history file format are:

- `id` - the history item ID
- `cmd` - the command text
- `cwd` - the current working directory at command execution, with `$HOME` replaced with `~`
- `exit` - the command’s exit code
- `dur` - execution duration in milliseconds
- `files` - the list of arguments that resolved to files, used for autosuggestion hinting
- `sid` - A session identifier unique to each fish instance, used to attach commands to each session

Additional metadata keys may be introduced in future versions.

## Writing records

fish will write records to the history file using `O_APPEND`. The kernel guarantees such writes are atomic up to some limit (typically ~4KB) which is more than sufficient for a single record.

## Vacuuming

Occasionally fish will _vacuum_ the file to reduce its size. Vacuuming involves:

1. Collapsing multiple records into a single record
2. Removing oldest items to enforce the history file size limit

Vacuuming occurs automatically, by writing an adjacent file and atomically moving it into place. The maximun record count is 1 million.
