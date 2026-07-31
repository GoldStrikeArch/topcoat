//! The dashboard's feed: fake stock ticks over server-sent events.
//!
//! One tick moves one symbol. The symbols and the prices they open at are the
//! island's own [`SYMBOLS`] and [`START_CENTS`], so the page the server renders
//! and the stream the browser subscribes to cannot disagree about what a slot
//! means.
//!
//! # Why the numbers are integers
//!
//! A price is cents and a move is cents, both whole. The island renders them on
//! the server and again in the browser, and those two renders have to be the
//! same bytes or hydration is lost. Integers are what the two float printers
//! agree on exactly, so nothing here is ever a fraction.
//!
//! # Deterministic when seeded
//!
//! The generator is a seeded xorshift and holds no clock, so a seed picks a
//! stream: the same `?seed=` gives the same symbols, prices and moves in the
//! same order every time. That is what lets a check assert a tick rather than a
//! shape, and it is why the seed is a query parameter rather than a constant.

use std::time::Duration;

use futures_core::Stream;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use topcoat::{
    Result,
    context::Cx,
    router::{
        content::sse::{Event, KeepAlive, Sse, last_event_id},
        error::RouterErrorExt,
        parse_query_params, route,
    },
};

use crate::dashboard_island::{START_CENTS, SYMBOLS};

/// The stream a request that names no seed gets.
const DEFAULT_SEED: u64 = 7;

/// How long the feed waits between ticks when a request does not say.
const DEFAULT_EVERY_MS: u64 = 900;

/// The largest move a single tick makes, in cents.
///
/// Big enough that the movers list reorders within a few ticks, which is what
/// the page is there to show.
const SWING_CENTS: i32 = 200;

/// The lowest a price is allowed to fall.
///
/// A price crossing zero would render a minus sign the server never wrote, and
/// the interesting behaviour is the reordering rather than the arithmetic.
const FLOOR_CENTS: i32 = 100;

/// One move of one symbol.
///
/// `slot` is the index of the symbol in [`SYMBOLS`], and it is what the browser
/// reads: it addresses the signal to write without either side holding a copy of
/// the symbol table. `symbol` rides along so the stream is legible to a person
/// reading it with `curl`.
#[derive(Serialize)]
struct Tick {
    slot: usize,
    symbol: &'static str,
    price: i32,
    delta: i32,
}

/// The prices, and the state the next move comes out of.
struct Feed {
    state: u64,
    prices: [i32; SYMBOLS.len()],
}

impl Feed {
    /// A feed at the opening prices, whose moves are decided by `seed`.
    fn new(seed: u64) -> Self {
        // A zero state is xorshift's one fixed point and would answer zero for
        // ever, so the seed is forced odd rather than rejected.
        Self {
            state: seed | 1,
            prices: START_CENTS,
        }
    }

    /// The next number in the sequence.
    fn step(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// The next move, applied.
    fn tick(&mut self) -> Tick {
        let slot = (self.step() % SYMBOLS.len() as u64) as usize;
        let swing = (self.step() % (SWING_CENTS as u64 * 2 + 1)) as i32 - SWING_CENTS;
        let was = self.prices[slot];
        let price = (was + swing).max(FLOOR_CENTS);

        // The move reported is the one that happened, which is not the one drawn
        // when the floor clamped it.
        self.prices[slot] = price;
        Tick {
            slot,
            symbol: SYMBOLS[slot],
            price,
            delta: price - was,
        }
    }
}

/// How a request picks its stream.
///
/// Read with [`parse_query_params`] rather than through a `#[query_params]`
/// accessor: that one hands back a reference borrowed from the request, and this
/// route's response is a stream that must borrow nothing.
#[derive(Deserialize)]
struct FeedQuery {
    seed: Option<u64>,
    every: Option<u64>,
}

/// The tick feed.
///
/// Each event is named `tick`, carries the tick as JSON, and is numbered, so a
/// browser that reconnects says where it got to and the feed resumes there
/// instead of replaying the series from the opening prices.
#[route(GET "/demo/ticks")]
async fn ticks(cx: &Cx) -> Result<Sse<impl Stream<Item = Result<Event>> + use<>>> {
    let query = parse_query_params::<FeedQuery>(cx)
        .ok_or_bad_request("`seed` and `every` are whole numbers")?;
    let every = Duration::from_millis(query.every.unwrap_or(DEFAULT_EVERY_MS));
    let resume = last_event_id(cx)
        .and_then(|id| id.parse::<u64>().ok())
        .map_or(0, |last| last + 1);

    // Resuming winds the generator forward rather than seeking it: the sequence
    // is only defined by having been walked, and a client that reconnects is
    // owed the prices it would have had.
    let mut feed = Feed::new(query.seed.unwrap_or(DEFAULT_SEED));
    for _ in 0..resume {
        feed.tick();
    }

    let events = stream::unfold((feed, resume), move |(mut feed, id)| async move {
        tokio::time::sleep(every).await;
        let event = Event::new().event("tick").id(id.to_string()).retry(every);
        Some((event.json_data(&feed.tick()), (feed, id + 1)))
    });

    Ok(Sse::new(events).keep_alive(KeepAlive::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_picks_a_stream() {
        // Not named `ticks`: the route above is a constant of that name, and a
        // `let` whose name is a constant in scope is a pattern match against it.
        let walk = |seed| {
            let mut feed = Feed::new(seed);
            (0..8)
                .map(|_| feed.tick())
                .map(|tick| (tick.slot, tick.price))
                .collect::<Vec<_>>()
        };

        assert_eq!(walk(DEFAULT_SEED), walk(DEFAULT_SEED));
        assert_ne!(walk(DEFAULT_SEED), walk(DEFAULT_SEED + 1));
    }

    #[test]
    fn a_tick_reports_the_move_it_made() {
        let mut feed = Feed::new(DEFAULT_SEED);
        let mut prices = START_CENTS;

        for _ in 0..200 {
            let tick = feed.tick();
            assert_eq!(tick.symbol, SYMBOLS[tick.slot]);
            assert_eq!(tick.price - prices[tick.slot], tick.delta);
            assert!(tick.price >= FLOOR_CENTS);
            prices[tick.slot] = tick.price;
        }
    }

    #[test]
    fn resuming_lands_on_the_price_the_stream_had_reached() {
        let mut whole = Feed::new(DEFAULT_SEED);
        let skipped = (0..5).map(|_| whole.tick()).count();
        let next = whole.tick();

        let mut resumed = Feed::new(DEFAULT_SEED);
        for _ in 0..skipped {
            resumed.tick();
        }

        let tick = resumed.tick();
        assert_eq!(
            (tick.slot, tick.price, tick.delta),
            (next.slot, next.price, next.delta)
        );
    }

    #[test]
    fn every_symbol_moves_within_a_reasonable_stretch() {
        let mut feed = Feed::new(DEFAULT_SEED);
        let mut seen = [false; SYMBOLS.len()];

        for _ in 0..100 {
            seen[feed.tick().slot] = true;
        }

        assert!(seen.iter().all(|moved| *moved), "{seen:?}");
    }
}
