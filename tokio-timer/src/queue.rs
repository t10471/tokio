#![allow(warnings)]

//! TODO: Dox

use {Error, Delay};
use clock::now;
use wheel::{self, Wheel};

use futures::{Future, Stream, Poll};
use slab::Slab;

use std::cmp;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

/// TODO: Dox
#[derive(Debug)]
pub struct DelayQueue<T> {
    /// Stores data associated with entries
    slab: Slab<Entry<T>>,

    /// Lookup structure tracking all delays in the queue
    wheel: Wheel<Stack<T>>,

    /// Delays that were inserted when already expired. These cannot be stored
    /// in the wheel
    expired: Stack<T>,

    /// Delay expiring when the *first* item in the queue expires
    delay: Option<Delay>,

    /// Wheel polling state
    poll: wheel::Poll,

    /// Instant at which the timer starts
    start: Instant,
}

/// TODO: Dox
#[derive(Debug)]
pub struct Key {
    index: usize,
}

#[derive(Debug)]
struct Stack<T> {
    /// Head of the stack
    head: Option<usize>,
    _p: PhantomData<T>,
}

#[derive(Debug)]
struct Entry<T> {
    /// The data being stored in the queue and will be returned at the requested
    /// instant.
    inner: T,

    /// The instant at which the item is returned.
    when: u64,

    /// Next entry in the stack
    next: Option<usize>,

    /// Previous entry in the stac
    prev: Option<usize>,
}

impl<T> DelayQueue<T> {
    /// TODO: Dox
    pub fn new() -> DelayQueue<T> {
        DelayQueue {
            wheel: Wheel::new(),
            slab: Slab::new(),
            expired: Stack::default(),
            delay: None,
            poll: wheel::Poll::new(0),
            start: now(),
        }
    }

    /// TODO: Dox
    pub fn insert(&mut self, value: T, when: Instant) -> Key {
        use self::wheel::{InsertError, Stack};

        // Normalize the deadline. Values cannot be set to expire in the past.
        let when = self.normalize_deadline(when);

        // Insert the value in the store
        let key = self.slab.insert(Entry {
            inner: value,
            when,
            next: None,
            prev: None,
        });

        // Register the deadline with the timer wheel
        match self.wheel.insert(when, key, &mut self.slab) {
            Ok(_) => {}
            Err((_, InsertError::Elapsed)) => {
                // The delay is already expired, store it in the expired queue
                self.expired.push(key, &mut self.slab);
            }
            Err((_, err)) => {
                panic!("invalid deadline; err={:?}", err)
            }
        }

        Key::new(key)
    }

    /// TODO: Dox
    pub fn remove(&mut self, key: Key) -> T {
        unimplemented!();
    }

    /// TODO: Dox
    pub fn reset(&mut self, key: &Key, when: Instant) {
        unimplemented!();
    }

    /// TODO: Dox
    pub fn clear(&mut self) {
        unimplemented!();
    }

    fn poll_idx(&mut self) -> Poll<Option<usize>, Error> {
        use self::wheel::Stack;

        let expired = self.expired.pop(&mut self.slab);

        if expired.is_some() {
            return Ok(expired.into());
        }

        loop {
            if let Some(ref mut delay) = self.delay {
                if !delay.is_elapsed() {
                    try_ready!(delay.poll());
                }

                let now = ::ms(delay.deadline() - self.start, ::Round::Down);

                self.poll = wheel::Poll::new(now);
            }

            self.delay = None;

            if let Some(idx) = self.wheel.poll(&mut self.poll, &mut self.slab) {
                return Ok(Some(idx).into());
            }

            let deadline = match self.wheel.poll_at(&self.poll) {
                Some(poll_at) => {
                    self.start + Duration::from_millis(poll_at)
                }
                None => return Ok(None.into()),
            };

            self.delay = Some(Delay::new(deadline));
        }
    }

    fn normalize_deadline(&self, when: Instant) -> u64 {
        let when = if when < self.start {
            0
        } else {
            ::ms(when - self.start, ::Round::Up)
        };

        cmp::max(when, self.wheel.elapsed())
    }
}

impl<T> Stream for DelayQueue<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<T>, Error> {
        let item = try_ready!(self.poll_idx())
            .map(|idx| {
                let entry = self.slab.remove(idx);
                debug_assert!(entry.next.is_none());
                debug_assert!(entry.prev.is_none());
                entry.inner
            });

        Ok(item.into())
    }
}

impl<T> wheel::Stack for Stack<T> {
    type Owned = usize;
    type Borrowed = usize;
    type Store = Slab<Entry<T>>;

    fn is_empty(&self, _store: &Self::Store) -> bool {
        self.head.is_none()
    }

    fn push(&mut self, item: Self::Owned, store: &mut Self::Store) {
        // Ensure the entry is not already in a stack.
        debug_assert!(store[item].next.is_none());
        debug_assert!(store[item].prev.is_none());

        // Remove the old head entry
        let old = self.head.take();

        if let Some(idx) = old {
            store[idx].prev = Some(item);
        }

        store[item].next = old;
        self.head = Some(item)
    }

    fn pop(&mut self, store: &mut Self::Store) -> Option<Self::Owned> {
        if let Some(idx) = self.head {
            self.head = store[idx].next;

            if let Some(idx) = self.head {
                store[idx].prev = None;
            }

            store[idx].next = None;
            debug_assert!(store[idx].prev.is_none());

            Some(idx)
        } else {
            None
        }
    }

    fn remove(&mut self, item: &Self::Borrowed, store: &mut Self::Store) {
        // Ensure that the entry is in fact contained by the stack
        debug_assert!({
            // This walks the full linked list even if an entry is found.
            let mut next = self.head;
            let mut contains = false;

            while let Some(idx) = next {
                if idx == *item {
                    debug_assert!(contains);
                    contains = true;
                }

                next = store[idx].next;
            }

            contains
        });

        if let Some(next) = store[*item].next {
            store[next].prev = store[*item].prev;
        }

        if let Some(prev) = store[*item].prev {
            store[prev].next = store[*item].next;
        }

        store[*item].next = None;
        store[*item].prev = None;
    }

    fn when(item: &Self::Borrowed, store: &Self::Store) -> u64 {
        store[*item].when
    }
}

impl<T> Default for Stack<T> {
    fn default() -> Stack<T> {
        Stack {
            head: None,
            _p: PhantomData,
        }
    }
}

impl Key {
    pub(crate) fn new(index: usize) -> Key {
        Key { index }
    }
}
