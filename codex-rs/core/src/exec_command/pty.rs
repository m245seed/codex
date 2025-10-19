#[cfg(windows)]
use anyhow::Context;
use portable_pty::PtyPair;
use portable_pty::PtySize;

#[cfg(windows)]
pub(crate) fn open_pty(size: PtySize) -> anyhow::Result<PtyPair> {
    use portable_pty::PtySystem;

    portable_pty::windows::conpty::ConPtySystem::default()
        .openpty(size)
        .with_context(|| {
            "failed to create Windows ConPTY pseudoterminal; ensure the host supports ConPTY"
                .to_string()
        })
}

#[cfg(not(windows))]
pub(crate) fn open_pty(size: PtySize) -> anyhow::Result<PtyPair> {
    portable_pty::native_pty_system().openpty(size)
}
