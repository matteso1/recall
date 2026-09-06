//! Keep each queued rune write attached to the identity that requested it.
use tokio::sync::{Mutex, MutexGuard};

pub(crate) async fn acquire<T, E>(
    writes: &Mutex<()>,
    capture: impl FnOnce() -> Result<T, E>,
) -> Result<(T, MutexGuard<'_, ()>), E> {
    let identity = capture()?;
    let write = writes.lock().await;
    // The caller revalidates this original identity after acquiring the write slot.
    Ok((identity, write))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{poll_fn, Future};
    use std::task::Poll;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Identity {
        generation: u64,
        champion: &'static str,
        rune_signature: &'static str,
    }

    #[tokio::test]
    async fn queued_rune_request_cannot_rebind_after_champion_returns_in_a_new_generation() {
        let writes = Mutex::new(());
        let blocked = writes.lock().await;
        let current = std::sync::Mutex::new(Identity {
            generation: 7,
            champion: "Ahri",
            rune_signature: "domination:sorcery:unchanged-perks",
        });
        let queued = acquire(&writes, || Ok::<_, ()>(current.lock().unwrap().clone()));
        tokio::pin!(queued);
        // Poll once while the write slot is held, guaranteeing this request really is queued.
        poll_fn(|context| {
            assert!(queued.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;

        *current.lock().unwrap() = Identity {
            generation: 8,
            champion: "Lulu",
            rune_signature: "sorcery:resolve:different-perks",
        };
        *current.lock().unwrap() = Identity {
            generation: 9,
            champion: "Ahri",
            rune_signature: "domination:sorcery:unchanged-perks",
        };
        drop(blocked);

        let (captured, _write) = queued.await.unwrap();
        assert_eq!(
            captured.generation, 7,
            "a queued request must retain its original match generation"
        );
        let latest = current.lock().unwrap();
        assert_eq!(captured.champion, latest.champion);
        assert_eq!(captured.rune_signature, latest.rune_signature);
        assert_ne!(
            captured.generation, latest.generation,
            "post-lock identity validation must reject the old request even when champion and runes match"
        );
    }
}
