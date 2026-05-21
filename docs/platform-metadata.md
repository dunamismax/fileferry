# Platform Metadata

FileFerry captures platform metadata so restores can be honest and repeatable
across Windows, macOS, Linux, FreeBSD, and NetBSD. Metadata records are stored
inside encrypted snapshot manifests or encrypted metadata objects; source paths,
file names, ownership, attributes, xattrs, ACLs, and platform-specific details
must not appear in plaintext repository objects.

This document defines the v1 metadata target. Implementation must still prove
behavior with platform-specific tests before any platform is called supported.

## Capture Model

Every filesystem entry records:

- Entry kind: regular file, directory, symlink, and explicitly handled special
  file kinds.
- Portable mode facts: readability/writability/executability where the source
  platform exposes them, plus POSIX mode bits on Unix-like systems.
- Size for regular files and symlink target length where available.
- Modification time and creation/birth time where the source platform exposes
  it.
- Ownership identifiers: numeric UID/GID on Unix-like systems and Windows owner
  SID when available.
- Symlink target as metadata, not followed file content.
- Sparse-file extent information where the source platform and filesystem
  expose it reliably.

Platform extensions are captured as namespaced records:

- `windows`: file attributes, reparse point kind, alternate data stream names
  and contents when enabled, owner SID, DACL/SACL capture status, and long-path
  spelling used for the source entry.
- `macos`: POSIX mode/ownership, file flags, birth time, xattrs, resource fork
  if present, Finder metadata exposed through xattrs, and filename normalization
  observations.
- `linux`: POSIX mode/ownership, file type, timestamps, xattrs, ACL capture
  status, sparse extents, and special-file identifiers when explicitly enabled.
- `freebsd`: POSIX mode/ownership, file flags, timestamps, xattrs where
  available, ACL capture status, sparse extents, and special-file identifiers
  when explicitly enabled.
- `netbsd`: POSIX mode/ownership, file flags where available, timestamps, xattrs
  where available, ACL capture status, sparse extents, and special-file
  identifiers when explicitly enabled.

Metadata capture must distinguish three states:

- Captured: the value was read and stored.
- Unsupported: the source platform or filesystem does not expose the field.
- Denied: the field exists but permissions prevented capture.

Denied and unsupported metadata are not fatal to backup by default, but they
must be surfaced as warnings in human output and as structured JSON/JSONL
events. A strict mode may promote those warnings to failure.

## Restore Behavior

Restore applies content first, then portable metadata, then platform-specific
metadata that the destination can represent.

Current implementation status: initialized local and S3-compatible repository
restores apply captured modified timestamps for restored regular files and
directories after content writes and verification. On Unix destinations,
restores also apply captured regular-file and directory permission bits
(`0o777`) where representable, after destination entries have been written.
Restores verify captured Unix UID/GID ownership for restored regular files and
directories after writes and warn when the destination ownership does not
match, but they do not call `chown`.
Non-dry-run restores preflight selected manifest entries for destination path
case collisions when the destination filesystem can be observed as
case-insensitive. Windows destinations reject selected paths with Windows
reserved-name segments before destination writes; this is a guardrail, not a
claim that Windows restore support has met the release bar.
Dry-run restore reports the count of regular-file and directory modified
timestamp fields selected for restore, regular-file and directory
creation/birth timestamp fields selected for warning, captured Unix permission
fields selected for restore, captured Unix ownership fields selected for
restore, and selected symlink timestamp plus captured Unix symlink metadata
fields. It also reports selected reportable xattr status fields when xattrs
were observed or xattr capture was denied, and selected ACL status fields when
ACLs were observed or ACL capture was denied in constructed or future
manifests, plus selected file flag status fields when file flags were observed
or file flag capture was denied in constructed or future manifests, plus
selected resource fork status fields when resource forks were observed or
resource fork capture was denied in constructed or future manifests, plus
selected Windows attribute status fields when Windows attributes were observed
or Windows attribute capture was denied in constructed or future manifests,
plus selected sparse extent status fields when sparse extents were observed or
sparse extent capture was denied in constructed or future manifests. It can
surface denied, unsupported, invalid, unrepresentable, or
not-yet-restored metadata warnings without writing destination entries.
Captured entry metadata records the source platform for new manifests; older
v0 manifests that lack this field are read as `unknown`. Current restores do
not restore symlink timestamps, symlink Unix mode/ownership, creation/birth
time for regular files or directories, Unix ownership by changing destination
owners, Unix special mode bits, ACLs, xattr values, resource forks, Windows
attributes, BSD flags, sparse extents, or other platform-specific metadata
yet. Selected regular-file and directory creation/birth timestamps are
reported as structured `portable`/`created` metadata warnings because this
version does not restore them. New manifests also record reportable xattr
presence/count status where the destination build and filesystem expose xattr
listing; xattr names and values are not restored by this version. The observed
macOS `com.apple.provenance` implementation detail is not counted as a
reportable xattr. New manifests also have ACL status scaffolding, but current
capture records ACL status as unsupported and does not read ACL names,
entries, permissions, or values. New manifests also have file flag status
scaffolding, but current capture records file flag status as unsupported and
does not read or restore file flag values. New manifests also have Windows
attribute status scaffolding, but current capture records Windows attribute
status as unsupported and does not read or restore Windows attribute values.
New manifests also have resource fork status scaffolding, but current capture
records resource fork status as unsupported and does not read or restore
resource fork values. New manifests also have sparse extent status
scaffolding, but current capture records sparse extent status as unsupported
and does not read or restore sparse extent maps. A selected timestamp, Unix
mode, Unix ownership, creation/birth timestamp, symlink metadata field, xattr
status field, ACL status field, file flag status field, resource fork status
field, Windows attribute status field, or sparse extent status field that
could not be applied, represented, or restored by this version is reported as
a metadata warning with entry id, metadata namespace, field, source platform,
destination platform, and reason; a restore with only metadata warnings returns
partial-success exit code `10`.

When metadata cannot be represented on the destination platform, FileFerry must:

- Restore file content and directory structure whenever doing so is safe.
- Skip the unrepresentable metadata field without silently pretending success.
- Emit a human warning on stderr.
- Emit a machine-readable item-level warning that includes the snapshot entry
  id, metadata namespace, metadata field, source platform, destination platform,
  and reason.
- Return partial-success exit code `10` when the restore otherwise succeeds but
  one or more requested metadata fields could not be applied.

When metadata application is denied by destination permissions, FileFerry must
report a permission failure for that field. Strict restore mode may fail the
whole restore; default restore records partial success when file content was
restored correctly.

Restore must not invent destination metadata to mimic unsupported source
metadata. If exact metadata restoration is impossible, the report needs to say
which field was skipped and why.

## Safety Rules

- Symlinks are restored as symlinks by default and must not be followed during
  restore writes.
- Special files require explicit opt-in before creation.
- Ownership, ACLs, file flags, Windows attributes, xattrs, resource forks,
  sparse extents, and alternate data streams are restored only after path
  destination checks pass.
- Case collisions and reserved names must be detected before writes begin.
- Timestamp restoration happens after content writes and fsync where practical.
- Any metadata parser failure after decryption is a repository integrity error,
  not a best-effort warning.

## Reference Points

The v1 implementation should verify API choices against current primary
documentation before coding each platform path:

- Microsoft Win32 file attribute constants and related file information APIs.
- Apple filesystem metadata, URL resource values, and file manager attribute
  documentation.
- Linux `stat`, `statx`, xattr, ACL, and sparse-file interfaces.
- FreeBSD `stat`, flags, xattr/extattr, ACL, and sparse-file interfaces.
- NetBSD `stat`, flags, extended attribute, ACL, and sparse-file interfaces.
