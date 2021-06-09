use std::{pin::Pin, task::Poll, time::Duration};

use futures::{future::pending, Future, FutureExt, Stream};
use tokio::{
    select,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
    time::{sleep, Instant},
};

use crate::error::StomperError;

enum ResettableTimerCommand {
    Reset,
    ChangePeriod(Duration),
}

/// Provides the API for resetting or changing the period of it's associated ResettableTimer
pub struct ResettableTimerResetter {
    sender: UnboundedSender<ResettableTimerCommand>,
}

impl ResettableTimerResetter {
    /// Resets the timer, so that the next time the timer emits will be `now + period`
    pub fn reset(&self) -> Result<(), StomperError> {
        self.sender
            .send(ResettableTimerCommand::Reset)
            .map(|_| ())
            .map_err(|_| StomperError::new("Error resetting stream"))
    }

    /// Changes the period of the timer.
    ///
    /// This will also reset the timer, so that the next emission will be at `now + new_period`
    pub fn change_period(&self, new_period: Duration) -> Result<(), StomperError> {
        self.sender
            .send(ResettableTimerCommand::ChangePeriod(new_period))
            .map(|_| ())
            .map_err(|_| StomperError::new("Error updating stream period"))
    }
}

/// A timer, implemented as as a stream of `unit` emissions, emitting each time `period` has elapsed.
///
/// The timer can be reset, so that the period will begin a new from `now`, and the period can be changed, which also causes a reset.
pub struct ResettableTimer {
    period: Duration,
    receiver: Option<UnboundedReceiver<ResettableTimerCommand>>,
    task: Option<JoinHandle<StreamState>>,
}

#[derive(Debug)]
enum StreamState {
    /// Indicates whether the stream has fired, and what the new period is
    Fired(JoinHandle<StreamState>),
}

impl ResettableTimer {
    /// Creates a new timer with the given `period`. Note that the timer will not start until it is first polled.
    ///
    /// Both the timer and the resetter allowing the timer to be reset or modified are returned
    pub fn create(period: Duration) -> (Self, ResettableTimerResetter) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            ResettableTimer {
                period,
                receiver: Some(receiver),
                task: None,
            },
            ResettableTimerResetter { sender },
        )
    }

    /// Creates a new timer with an infinite `period`. This is useful when a timer is required prior to knowledge about
    /// what its period should be.
    pub fn default() -> (Self, ResettableTimerResetter) {
        Self::create(Duration::from_millis(0))
    }

    fn create_task_no_receiver(
        period: Duration,
    ) -> Pin<Box<dyn Future<Output = StreamState> + Send>> {
        // The period can never be changed, the sleep never reset; so just sleep
        sleep(period)
            .map(move |_| {
                StreamState::Fired(tokio::task::spawn(
                    ResettableTimer::create_task_no_receiver(period).boxed(),
                ))
            })
            .boxed()
    }

    fn create_task_with_receiver(
        period: Duration,
        receiver: UnboundedReceiver<ResettableTimerCommand>,
    ) -> Pin<Box<dyn Future<Output = StreamState> + Send>> {
        async move {
            let period = period;
            let mut receiver = receiver;
            let mut sleep = Box::pin(sleep(period));

            let receive = receiver.recv();

            // reset the timer; not strictly necessary first iteration, but not harm
            sleep.as_mut().reset(Instant::now() + period);

            let command_to_new_period = |period, command| {
                if let ResettableTimerCommand::ChangePeriod(new_period) = command {
                    new_period
                } else {
                    period
                }
            };

            // If the period is not set to a positive number, wait only for events that change it
            if period.as_millis() == 0 {
                match receive.await {
                    None => pending::<StreamState>().await, // sender was dropped, so no new event will be received or emitted; wait indefinitely
                    Some(command) => {
                        ResettableTimer::create_task_with_receiver(
                            command_to_new_period(period, command),
                            receiver,
                        )
                        .await
                    }
                }
            } else {
                select! {
                    _ = &mut sleep => {
                        StreamState::Fired(tokio::task::spawn(ResettableTimer::create_task_with_receiver(period, receiver).boxed()))
                    }

                    received = receive => match received {
                        None => ResettableTimer::create_task_no_receiver(period).await, // wait again, but this time without being able to reset
                        Some(command) => ResettableTimer::create_task_with_receiver(command_to_new_period(period, command), receiver).await,
                    }
                }
            }
        }.boxed()
    }

    fn poll_existing_task(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Option<()>> {
        self.task
            .as_mut()
            .unwrap()
            .poll_unpin(cx) // Poll the task
            .map(|state| {
                // Transform the result appropriately...
                match state {
                    Ok(StreamState::Fired(new_task)) => {
                        // ... updating our own state along the way
                        self.task.replace(new_task);
                        Some(())
                    }
                    _ => None,
                }
            })
    }

    fn initialise(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Option<()>> {
        let receiver = self
            .receiver
            .take()
            .expect("Bad state: neither remote not receiver present");

        let task =
            ResettableTimer::create_task_with_receiver(self.period.clone(), receiver).boxed();

        self.task.replace(tokio::task::spawn(task));
        // poll the task so that it will trigger the waker if it gets done
        self.task.as_mut().unwrap().poll_unpin(cx).map(|_| None)
    }
}

impl Drop for ResettableTimer {
    fn drop(&mut self) {
        self.task.take().unwrap().abort();
    }
}

impl Stream for ResettableTimer {
    type Item = ();

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let stream = self.get_mut();

        if stream.task.is_some() {
            stream.poll_existing_task(cx)
        } else {
            stream.initialise(cx)
        }
    }
}

#[cfg(test)]
mod test {

    use futures::{FutureExt, StreamExt};
    use tokio::{
        task::yield_now,
        time::{pause, resume, sleep},
    };

    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum State {
        Fired,
        NotFired,
    }

    async fn sleep_and_wait(stream: &mut ResettableTimer, millis: u64) -> State {
        pause();
        sleep(Duration::from_millis(millis)).await;
        resume();

        match stream.next().now_or_never() {
            None => State::NotFired,
            _ => State::Fired,
        }
    }

    #[tokio::test]
    async fn it_does_not_fire_if_not_elapsed() {
        let (mut stream, _) = ResettableTimer::create(Duration::from_millis(5000));

        delay_expecting_not_fired(&mut stream, 4800).await;
    }

    #[tokio::test]
    async fn it_fires_if_elapsed() {
        let (mut stream, _) = ResettableTimer::create(Duration::from_millis(5000));

        stream.next().now_or_never(); // Kick off the timer.

        delay_expecting_fired(&mut stream, 6000).await;
    }

    #[tokio::test]
    async fn it_keeps_firing() {
        let (mut stream, _) = ResettableTimer::create(Duration::from_millis(500));
        stream.next().now_or_never(); // Kick off the timer.

        delay_expecting_fired(&mut stream, 600).await;
        delay_expecting_fired(&mut stream, 500).await;
        delay_expecting_fired(&mut stream, 500).await;
        delay_expecting_fired(&mut stream, 500).await;
        delay_expecting_fired(&mut stream, 500).await;
        delay_expecting_not_fired(&mut stream, 300).await;
        delay_expecting_fired(&mut stream, 300).await;
    }

    async fn delay_expecting_fired(stream: &mut ResettableTimer, millis: u64) {
        assert_eq!(State::Fired, sleep_and_wait(stream, millis).await);
    }
    async fn delay_expecting_not_fired(stream: &mut ResettableTimer, millis: u64) {
        assert_eq!(State::NotFired, sleep_and_wait(stream, millis).await);
    }

    #[tokio::test]
    async fn it_fires_later_if_reset() {
        let (mut stream, resetter) = ResettableTimer::create(Duration::from_millis(5000));
        stream.next().now_or_never(); // Kick off the timer.

        delay_expecting_not_fired(&mut stream, 4000).await;
        resetter.reset().expect("Unexpected error");
        yield_now().await;
        delay_expecting_not_fired(&mut stream, 2000).await;
        delay_expecting_fired(&mut stream, 3050).await;
    }

    #[tokio::test]
    async fn it_stays_on_new_schedule_after_reset() {
        let (mut stream, resetter) = ResettableTimer::create(Duration::from_millis(5000));

        delay_expecting_not_fired(&mut stream, 4000).await;
        resetter.reset().expect("Unexpected error");
        yield_now().await;
        delay_expecting_not_fired(&mut stream, 2000).await;
        delay_expecting_fired(&mut stream, 3050).await;
        delay_expecting_not_fired(&mut stream, 4000).await;
        delay_expecting_fired(&mut stream, 2000).await;
        delay_expecting_fired(&mut stream, 5000).await;
        delay_expecting_fired(&mut stream, 5000).await;
    }

    #[tokio::test]
    async fn it_changes_period_and_resets() {
        let (mut stream, resetter) = ResettableTimer::create(Duration::from_millis(5000));
        stream.next().now_or_never(); // Kick off the timer.

        delay_expecting_fired(&mut stream, 6000).await;

        resetter
            .change_period(Duration::from_millis(7000))
            .expect("Unexpected Error");

        // Nothing after original period would have passed
        delay_expecting_not_fired(&mut stream, 4100).await;

        // Nothing after new period would have passed from first fired
        delay_expecting_not_fired(&mut stream, 2000).await;

        // Fired after new period duration from reset
        delay_expecting_fired(&mut stream, 1000).await;

        // New period is maintained
        delay_expecting_not_fired(&mut stream, 5000).await;
        delay_expecting_fired(&mut stream, 2000).await;
    }

    #[tokio::test]
    async fn it_ends_task_when_dropped() {
        let (mut stream, resetter) = ResettableTimer::create(Duration::from_millis(5000));
        stream.next().now_or_never(); // Kick off the timer

        drop(stream);

        resetter
            .reset()
            .expect_err("Should be an error because the other end is no longer listening");
    }
}
