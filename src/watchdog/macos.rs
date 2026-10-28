/// Returns how to ping the main thread. Work queued for it still runs while a menu is open, so only a real hang
/// stops the replies.
pub fn pinger() -> impl Fn() + Send + 'static {
    || dispatch2::DispatchQueue::main().exec_async(|| super::MAIN.alive())
}
