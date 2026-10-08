//! The numbers that shape hail's behaviour, in one place so they can be
//! reviewed together. Each says why it is what it is.

use std::time::Duration;

/// Bodies a hook injects per prompt, and roughly how many bytes. The rest
/// stay unread for the next prompt or `hail inbox`: a backlog (66 unread on
/// one seat at migration) would otherwise be cut by the harness's hook output
/// limit after it was claimed.
pub const DELIVER_BODIES: usize = 5;
pub const DELIVER_BYTES: usize = 8 * 1024;

/// Entries per brief section before "… N more"; `--all` lifts it.
pub const BRIEF_SHOWN: usize = 5;

/// A send shows in the sender's brief once it is this old with no receipt,
/// and leaves it (lapsed) after a week.
pub const PENDING_LATE: Duration = Duration::from_mins(2);
pub const PENDING_LAPSE: Duration = Duration::from_hours(168);

/// An `fyi` is quiet (not typed; it arrives with the next prompt) only when
/// the recipient's prompt hook has run this recently. Otherwise the typed
/// envelope is the only way it would ever be seen.
pub const QUIET_HOOKS_SEEN: Duration = Duration::from_hours(168);

/// A hold lapses this long after it is sent, and a block this long, unless
/// `--for` says otherwise; `--for` may not exceed the maximum. A hold is a
/// person's decision ("don't touch X while I redesign it"), not a lock: tools
/// serialize landing, installs and timing runs. A block reports an external
/// blocker, which tends to outlive a session. Records with no `expires:` lapse
/// at their `time:` plus the default for their kind.
pub const HOLD_DEFAULT: Duration = Duration::from_hours(8);
pub const BLOCK_DEFAULT: Duration = Duration::from_hours(168);
pub const HOLD_MAX: Duration = Duration::from_hours(168);

/// At migration, unread mail keyed by a live pane id goes to that pane's
/// seat only when it is this recent: older mail may have been meant for an
/// agent that has since left the pane.
pub const MIGRATE_RECENT: Duration = Duration::from_hours(48);

/// `hail gc` archives read mail older than this many days by default, and
/// `doctor` suggests it past this many read messages in one seat.
pub const GC_DAYS: u64 = 90;
pub const GC_DUE: usize = 5000;

/// A pipe on stdin gets this long to deliver its first byte (a body piped
/// from a command that is slow to start), and this long in all, so a
/// never-ending producer (`yes | hail …`) cannot hang a send.
pub const STDIN_FIRST_BYTE: Duration = Duration::from_secs(2);
pub const STDIN_TOTAL: Duration = Duration::from_secs(30);
/// A body larger than this is refused rather than read without bound.
pub const STDIN_MAX: usize = 16 << 20;

/// `hook-errors.log` rotates at this size.
pub const HOOK_LOG_MAX: u64 = 1 << 20;

/// Seconds as the epoch arithmetic in records wants them.
#[allow(clippy::cast_possible_wrap)] // every duration here is far below i64::MAX
pub const fn secs(d: Duration) -> i64 {
    d.as_secs() as i64
}
