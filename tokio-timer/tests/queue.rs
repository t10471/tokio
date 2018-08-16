extern crate futures;
extern crate tokio_executor;
extern crate tokio_timer;
extern crate tokio_mock_task;

#[macro_use]
mod support;
use support::*;

use tokio_timer::*;
use tokio_mock_task::MockTask;

use futures::Stream;

#[test]
fn single_immediate_delay() {
    mocked(|_timer, time| {
        let mut queue = DelayQueue::new();
        let _key = queue.insert("foo", time.now());

        assert_ready!(queue, Some("foo"));
        assert_ready!(queue, None);
    });
}

#[test]
#[ignore]
fn multi_immediate_delays() {
}

#[test]
fn single_short_delay() {
    mocked(|timer, time| {
        let mut queue = DelayQueue::new();
        let _key = queue.insert("foo", time.now() + ms(5));

        let mut task = MockTask::new();

        task.enter(|| {
            assert_not_ready!(queue);
        });

        turn(timer, ms(1));

        assert!(!task.is_notified());

        turn(timer, ms(5));

        assert!(task.is_notified());

        assert_ready!(queue, Some("foo"));
        assert_ready!(queue, None);
    });
}
