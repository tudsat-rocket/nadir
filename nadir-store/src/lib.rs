use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use mavspec::rust::spec::MessageSpec;

/// Window the rates in [`MessageSummary`] are averaged over.
const FREQ_WINDOW_SECS: i64 = 5;

/// (`system_id`, `component_id`, `message_id`, stored message type, `instance_value`)
///
/// Keyed by Rust type as well as by message ID, because a dialect extending `common` generates its
/// own type for every message it inherits: `common::CommandLong` and `rapid::CommandLong` are both
/// message 76, and a series holds one or the other. Queries that name a message match on the ID,
/// and so see every dialect's version of it at once.
type SeriesKey = (u8, u8, u32, TypeId, Option<i64>);

/// Every value is a `Vec<(DateTime<Utc>, M)>` for the single concrete `M` its key names.
type SeriesMap = HashMap<SeriesKey, Box<dyn Series>>;

/// One field of one message over time, oldest first.
type Points = Vec<(DateTime<Utc>, f64)>;

/// (series, field index, samples per chunk, sentinel)
///
/// The sentinel is part of the key because two panes can plot one field with different ones - the
/// propulsion pressures drop `u16::MAX`, the generic field browser does not - and both can be on
/// screen at once.
type ChunkKey = (SeriesKey, usize, usize, Option<u64>);

/// Chunks a plot has already asked for, in series order. Whole ones only, so the edges a window
/// cuts are never cached.
type ChunkMap = HashMap<ChunkKey, Vec<Chunk>>;

/// Below this a chunked read is not worth caching: the window is then at most a few tens of
/// thousands of samples, and a level built for it would cover the whole series to serve one frame.
const MIN_CACHED_STRIDE: usize = 64;

#[derive(Clone)]
pub struct Db {
    series: Arc<Mutex<SeriesMap>>,
    /// Always locked after `series`, and only where both are held.
    chunks: Arc<Mutex<ChunkMap>>,
    instance_fields: Arc<HashMap<u32, String>>,
    msg_names: Arc<HashMap<u32, String>>,
    /// Message name to its ID and field names, unioned over the dialects defining it. Taken from
    /// the definitions rather than from the series, so a query naming a message that has not
    /// arrived yet still tells a bad field name from a good one.
    msg_defs: Arc<HashMap<String, (u32, Vec<String>)>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("No {0} stored for this system")]
    NotFound(&'static str),
    #[error("Unknown message or field: {0}")]
    UnknownField(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInstance {
    pub field: String,
    pub value: i64,
}

/// One [`MessageSummary`] while it is still being accumulated over a message's dialect types.
#[derive(Default)]
struct SummaryRow {
    count: usize,
    recent: usize,
    last: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MessageSummary {
    pub msg_id: u32,
    pub name: String,
    pub instance: Option<MessageInstance>,
    pub count: usize,
    pub freq_hz: f32,
    pub last: DateTime<Utc>,
}

/// The supertraits are what let a message be stored behind [`Any`] and handed back out by value.
pub trait MessageExt: MessageSpec + Clone + Debug + Send + Sync + 'static {
    /// Field names, in wire order. Indices into this are what [`Self::field_f64`] takes.
    fn rows() -> &'static [&'static str];

    /// Numeric value of the message's `instance="true"` field, if any.
    fn instance_value(&self) -> Option<i64> {
        None
    }

    /// Name of the message's `instance="true"` field, if any.
    fn instance_field() -> Option<&'static str> {
        None
    }

    /// Numeric value of the field at `index`, or `None` for an array field, which has no single
    /// number to plot.
    fn field_f64(&self, index: usize) -> Option<f64>;

    /// As [`Self::field_f64`], with readings equal to `sentinel` dropped as "no reading".
    fn field_value(&self, index: usize, sentinel: Option<f64>) -> Option<f64> {
        self.field_f64(index).filter(|v| Some(*v) != sentinel)
    }

    /// Appends a clone of self to its series. The dialect enums dispatch to the inner variant, so
    /// what lands in the store is always a concrete message type.
    fn store(&self, db: &Db, system_id: u8, component_id: u8, received_at: DateTime<Utc>);
}

/// The type-erased face of a stored series: everything a query addressing a message by name needs.
///
/// Windowing, chunking and caching read through it, so they are compiled once rather than once per
/// message type. The impl below is all that is still monomorphised over the several hundred of
/// them, and it dominates the crate's build, so keep its methods short.
trait Series: Any + Send + Sync {
    fn samples(&self) -> usize;

    /// Samples received after `cutoff`, for the message rates in [`MessageSummary`].
    fn count_since(&self, cutoff: DateTime<Utc>) -> usize;

    fn window_range(
        &self,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Range<usize>;

    fn field_index(&self, field_name: &str) -> Option<usize>;

    fn chunk(&self, range: Range<usize>, index: usize, sentinel: Option<f64>) -> Chunk;

    /// Every sample in `range` as a plot point, undecimated.
    fn points(&self, range: Range<usize>, index: usize, sentinel: Option<f64>) -> Points;

    fn last_time(&self) -> Option<DateTime<Utc>>;

    fn last_debug(&self) -> Option<String>;
}

impl<M: MessageExt> Series for Vec<(DateTime<Utc>, M)> {
    fn samples(&self) -> usize {
        self.len()
    }

    fn count_since(&self, cutoff: DateTime<Utc>) -> usize {
        Db::window_range(self, Some(cutoff), None).len()
    }

    fn window_range(
        &self,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Range<usize> {
        Db::window_range(self, since, until)
    }

    fn field_index(&self, field_name: &str) -> Option<usize> {
        M::rows().iter().position(|row| *row == field_name)
    }

    fn chunk(&self, range: Range<usize>, index: usize, sentinel: Option<f64>) -> Chunk {
        Chunk::of(
            self[range]
                .iter()
                .filter_map(|(t, msg)| Some((*t, msg.field_value(index, sentinel)?))),
        )
    }

    fn points(&self, range: Range<usize>, index: usize, sentinel: Option<f64>) -> Points {
        self[range]
            .iter()
            .filter_map(|(t, msg)| Some((*t, msg.field_value(index, sentinel)?)))
            .collect()
    }

    fn last_time(&self) -> Option<DateTime<Utc>> {
        Some(self.last()?.0)
    }

    fn last_debug(&self) -> Option<String> {
        Some(format!("{:#?}", self.last()?.1))
    }
}

impl Db {
    pub fn init() -> Self {
        let protocol = mavspec::definitions::protocol();
        let dialects = [
            protocol.get_dialect_by_name("common").unwrap(),
            protocol.get_dialect_by_name("ardupilotmega").unwrap(),
            rapid_dialect::definitions::protocol()
                .get_dialect_by_canonical_name("rapid")
                .unwrap(),
        ];

        Self {
            series: Arc::new(Mutex::new(HashMap::new())),
            chunks: Arc::new(Mutex::new(HashMap::new())),
            instance_fields: Arc::new(
                dialects
                    .into_iter()
                    .flat_map(Self::collect_instance_fields)
                    .collect(),
            ),
            msg_names: Arc::new(
                dialects
                    .into_iter()
                    .flat_map(|dialect| {
                        dialect
                            .messages()
                            .into_iter()
                            .map(|message| (message.id(), message.name().to_owned()))
                    })
                    .collect(),
            ),
            msg_defs: Arc::new(dialects.into_iter().fold(
                HashMap::new(),
                |mut defs: HashMap<String, (u32, Vec<String>)>, dialect| {
                    for message in dialect.messages() {
                        let (_, fields) = defs
                            .entry(message.name().to_owned())
                            .or_insert_with(|| (message.id(), Vec::new()));

                        for field in message.fields() {
                            if !fields.iter().any(|known| known == field.name()) {
                                fields.push(field.name().to_owned());
                            }
                        }
                    }

                    defs
                },
            )),
        }
    }

    fn collect_instance_fields(dialect: &mavinspect::protocol::Dialect) -> HashMap<u32, String> {
        dialect
            .messages()
            .into_iter()
            .filter_map(|message: &mavinspect::protocol::Message| {
                let field = message.fields().iter().find(|f| f.instance())?;
                Some((message.id(), field.name().to_owned()))
            })
            .collect()
    }

    /// Appends one message to its series. Called by the generated [`MessageExt::store`] impls,
    /// which are what resolve `M` to a concrete type.
    pub fn push<M: MessageExt>(
        &self,
        system_id: u8,
        component_id: u8,
        received_at: DateTime<Utc>,
        msg: M,
    ) {
        let key = (
            system_id,
            component_id,
            msg.id(),
            TypeId::of::<M>(),
            msg.instance_value(),
        );

        let mut series = self.series.lock().unwrap();
        let stored: &mut dyn Any = &mut **series
            .entry(key)
            .or_insert_with(|| Box::new(Vec::<(DateTime<Utc>, M)>::new()));

        stored
            .downcast_mut::<Vec<(DateTime<Utc>, M)>>()
            .expect("a series key names the type that series holds")
            .push((received_at, msg));
    }

    /// Every series holding an `M` for this system, one per instance value.
    fn slices<M: MessageExt>(
        series: &SeriesMap,
        system_id: u8,
        component_id: u8,
    ) -> impl Iterator<Item = (&SeriesKey, &[(DateTime<Utc>, M)])> {
        let type_id = TypeId::of::<M>();

        series
            .iter()
            .filter(move |((sys, comp, _, ty, _), _)| {
                *sys == system_id && *comp == component_id && *ty == type_id
            })
            .filter_map(|(key, stored)| {
                let stored: &dyn Any = &**stored;
                let rows = stored.downcast_ref::<Vec<(DateTime<Utc>, M)>>()?;
                Some((key, rows.as_slice()))
            })
    }

    /// Where the part of a series within `(since, until]` sits in it.
    ///
    /// Binary search, which holds because a series is appended in receive order: live ingest is
    /// sequential, and a recording is read in file order.
    fn window_range<M>(
        rows: &[(DateTime<Utc>, M)],
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Range<usize> {
        let start = since.map_or(0, |since| rows.partition_point(|(t, _)| *t <= since));
        let end = until.map_or(rows.len(), |until| {
            rows.partition_point(|(t, _)| *t <= until)
        });

        start..end.max(start)
    }

    fn window<M>(
        rows: &[(DateTime<Utc>, M)],
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> &[(DateTime<Utc>, M)] {
        &rows[Self::window_range(rows, since, until)]
    }

    pub fn write_message<M: MessageExt>(&self, system_id: u8, component_id: u8, msg: &M) {
        self.write_message_at(system_id, component_id, msg, Utc::now());
    }

    pub fn write_message_at<M: MessageExt>(
        &self,
        system_id: u8,
        component_id: u8,
        msg: &M,
        received_at: DateTime<Utc>,
    ) {
        msg.store(self, system_id, component_id, received_at);
    }

    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn last_message<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Result<M, DbError> {
        let series = self.series.lock().unwrap();

        Self::slices::<M>(&series, system_id, component_id)
            .filter_map(|(_, rows)| rows.last())
            .max_by_key(|(t, _)| *t)
            .map(|(_, msg)| msg.clone())
            .ok_or_else(|| DbError::NotFound(std::any::type_name::<M>()))
    }

    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn last_message_filtered<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> Result<M, DbError> {
        let Some((_, value)) = instance else {
            return self.last_message(system_id, component_id);
        };

        let series = self.series.lock().unwrap();

        Self::slices::<M>(&series, system_id, component_id)
            .find(|((.., instance), _)| *instance == Some(value))
            .and_then(|(_, rows)| rows.last())
            .map(|(_, msg)| msg.clone())
            .ok_or_else(|| DbError::NotFound(std::any::type_name::<M>()))
    }

    pub fn all_messages<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
    ) -> Vec<(DateTime<Utc>, M)> {
        self.messages_since(system_id, component_id, None, None)
    }

    /// The oldest `limit` messages newer than `since`, or all of them if no limit is given.
    pub fn messages_since<M: MessageExt + Default>(
        &self,
        system_id: u8,
        component_id: u8,
        since: Option<DateTime<Utc>>,
        limit: Option<usize>,
    ) -> Vec<(DateTime<Utc>, M)> {
        let series = self.series.lock().unwrap();
        let limit = limit.unwrap_or(usize::MAX);

        let mut windows = Self::slices::<M>(&series, system_id, component_id)
            .map(|(_, rows)| Self::window(rows, since, None))
            .filter(|rows| !rows.is_empty());

        let Some(first) = windows.next() else {
            return Vec::new();
        };

        // The common case is a message with no instance field, and so a single series, already in
        // receive order and needing no merge.
        let Some(second) = windows.next() else {
            return first.iter().take(limit).cloned().collect();
        };

        let mut merged: Vec<(DateTime<Utc>, M)> = first
            .iter()
            .chain(second)
            .chain(windows.flatten())
            .cloned()
            .collect();
        merged.sort_by_key(|(t, _)| *t);
        merged.truncate(limit);

        merged
    }

    /// Number of stored messages of this type, over every instance of it.
    pub fn message_count<M: MessageExt + Default>(&self, system_id: u8, component_id: u8) -> usize {
        let series = self.series.lock().unwrap();
        Self::slices::<M>(&series, system_id, component_id)
            .map(|(_, rows)| rows.len())
            .sum()
    }

    /// One row per `(message_id, instance_value)` pair stored for the given system/component,
    /// sorted by message ID and instance value.
    pub fn message_summary(&self, system_id: u8, component_id: u8) -> Vec<MessageSummary> {
        let cutoff = Utc::now() - chrono::TimeDelta::seconds(FREQ_WINDOW_SECS);
        let series = self.series.lock().unwrap();

        // A message two dialects define is stored once per dialect type, and reported as one row.
        let mut rows: HashMap<(u32, Option<i64>), SummaryRow> = HashMap::new();
        for ((sys, comp, msg_id, _, instance_value), stored) in series.iter() {
            if *sys != system_id || *comp != component_id {
                continue;
            }
            let Some(last) = stored.last_time() else {
                continue;
            };

            let row = rows.entry((*msg_id, *instance_value)).or_default();
            row.count += stored.samples();
            row.recent += stored.count_since(cutoff);
            row.last = row.last.max(Some(last));
        }

        drop(series);

        #[allow(
            clippy::cast_precision_loss,
            reason = "a sample count is bounded by realistic message rates; precision loss is irrelevant for display"
        )]
        let mut result: Vec<MessageSummary> = rows
            .into_iter()
            .filter_map(|((msg_id, instance_value), row)| {
                Some(MessageSummary {
                    msg_id,
                    name: self
                        .msg_names
                        .get(&msg_id)
                        .cloned()
                        .unwrap_or_else(|| format!("UNKNOWN_{msg_id}")),
                    instance: instance_value.zip(self.instance_fields.get(&msg_id)).map(
                        |(value, field)| MessageInstance {
                            field: field.clone(),
                            value,
                        },
                    ),
                    count: row.count,
                    freq_hz: row.recent as f32 / FREQ_WINDOW_SECS as f32,
                    last: row.last?,
                })
            })
            .collect();

        result.sort_by_key(|row| (row.msg_id, row.instance.as_ref().map(|i| i.value)));

        result
    }

    /// Fetch the last message of a given name, pretty-printed via `Debug`. `None` when no dialect
    /// defines it.
    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn last_message_debug_by_name(
        &self,
        msg_name: &str,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> Result<Option<String>, DbError> {
        let Some((msg_id, _)) = self.msg_defs.get(msg_name) else {
            return Ok(None);
        };

        let series = self.series.lock().unwrap();

        Self::matching(&series, *msg_id, system_id, component_id, instance)
            .filter_map(|(_, stored)| Some((stored.last_time()?, stored)))
            .max_by_key(|(last, _)| *last)
            .and_then(|(_, stored)| stored.last_debug())
            .map(Some)
            .ok_or(DbError::NotFound("message"))
    }

    /// One field of one message type over time, for plotting.
    ///
    /// A message several dialects define is stored under whichever of their types decoded it, so
    /// one line's points can be spread across them, as they are across the instances of a
    /// multi-instance message. A plot is an aggregation rather than a typed read, so unlike
    /// `all_messages` those series simply merge.
    #[allow(
        clippy::unwrap_in_result,
        reason = "a poisoned store is not recoverable"
    )]
    pub fn timeseries_by_name(
        &self,
        msg_name: &str,
        field_name: &str,
        args: TimeseriesArgs<'_>,
    ) -> Result<Vec<(DateTime<Utc>, f64)>, DbError> {
        let (msg_id, fields) = self
            .msg_defs
            .get(msg_name)
            .ok_or_else(|| DbError::UnknownField(msg_name.to_owned()))?;

        if !fields.iter().any(|field| field == field_name) {
            return Err(DbError::UnknownField(field_name.to_owned()));
        }

        let series = self.series.lock().unwrap();
        let mut found = Self::matching(
            &series,
            *msg_id,
            args.system_id,
            args.component_id,
            args.instance,
        )
        // A shared message can have a different field set in each dialect; the ones without this
        // field contribute nothing rather than failing the whole line.
        .filter_map(|(key, stored)| {
            let index = stored.field_index(field_name)?;
            let window = stored.window_range(args.since, args.until);
            (!window.is_empty()).then_some((key, stored, index, window))
        });

        let (Some(first), second) = (found.next(), found.next()) else {
            return Ok(Vec::new());
        };

        let Some(second) = second else {
            let (key, stored, index, window) = first;
            return Ok(self.line(key, stored, index, window, args));
        };

        // Chunks of two series fall on different boundaries, and one can come back decimated while
        // the other is short enough to be exact, so a line spread across several has to be read
        // verbatim and decimated as a whole. Having no series of its own, the merge is chunked from
        // its own start rather than anchored, and cannot be cached.
        let mut points: Points = [first, second]
            .into_iter()
            .chain(found)
            .flat_map(|(_, stored, index, window)| stored.points(window, index, args.sentinel))
            .collect();
        points.sort_by_key(|(t, _)| *t);

        let Some(max_points) = args.max_points.filter(|budget| points.len() > *budget) else {
            return Ok(points);
        };

        let stride = Chunk::stride(points.len(), max_points);
        Ok(Chunk::assemble(0..points.len(), stride, &[], &|range| {
            Chunk::of(points[range].iter().copied())
        }))
    }

    /// Every series of one message for this system, one per dialect type and instance value.
    fn matching<'a>(
        series: &'a SeriesMap,
        msg_id: u32,
        system_id: u8,
        component_id: u8,
        instance: Option<(&str, i64)>,
    ) -> impl Iterator<Item = (&'a SeriesKey, &'a dyn Series)> {
        series
            .iter()
            .filter(move |((sys, comp, id, _, value), _)| {
                *sys == system_id
                    && *comp == component_id
                    && *id == msg_id
                    && instance.is_none_or(|(_, wanted)| *value == Some(wanted))
            })
            .map(|(key, stored)| (key, &**stored))
    }

    /// The visible part of one series as plot points, chunked down to the budget if it exceeds it.
    fn line(
        &self,
        key: &SeriesKey,
        series: &dyn Series,
        index: usize,
        window: Range<usize>,
        args: TimeseriesArgs<'_>,
    ) -> Points {
        let Some(max_points) = args.max_points.filter(|budget| window.len() > *budget) else {
            return series.points(window, index, args.sentinel);
        };

        let stride = Chunk::stride(window.len(), max_points);
        let chunk = |range| series.chunk(range, index, args.sentinel);

        if stride < MIN_CACHED_STRIDE {
            return Chunk::assemble(window, stride, &[], &chunk);
        }

        let mut cache = self.chunks.lock().unwrap();
        let chunks = cache
            .entry((*key, index, stride, args.sentinel.map(f64::to_bits)))
            .or_default();

        // Chunks are only ever appended, because a series is: what is already there covers the
        // same samples it did when it was built.
        for i in chunks.len()..series.samples() / stride {
            chunks.push(chunk(i * stride..(i + 1) * stride));
        }

        Chunk::assemble(window, stride, chunks, &chunk)
    }
}

/// What a run of samples contributes to the drawn line: its extremes, and where it was interrupted.
#[derive(Clone, Copy, Default)]
struct Chunk {
    min: Option<(DateTime<Utc>, f64)>,
    max: Option<(DateTime<Utc>, f64)>,
    nan_at: Option<DateTime<Utc>>,
}

impl Chunk {
    /// How many samples one chunk covers, for a window of `samples` against a budget of
    /// `max_points`. A power of two, so that resizing a pane wanders between few enough sizes for
    /// [`Db::chunks`] to be worth keeping.
    fn stride(samples: usize, max_points: usize) -> usize {
        (samples / (max_points / 2).max(1))
            .max(1)
            .next_power_of_two()
    }

    fn of(points: impl Iterator<Item = (DateTime<Utc>, f64)>) -> Self {
        let mut chunk = Self::default();

        for (t, v) in points {
            // A NaN is a deliberate gap, and `f64::min`/`max` would swallow it and draw straight
            // through. Holding the extremes in an Option rather than seeding them with infinities
            // matters for the same reason: a chunk with no finite sample would otherwise emit one,
            // and a non-finite y bound makes egui_plot reset the whole plot to [-1, 1].
            if v.is_nan() {
                chunk.nan_at = chunk.nan_at.or(Some(t));
                continue;
            }

            if chunk.min.is_none_or(|(_, m)| v < m) {
                chunk.min = Some((t, v));
            }
            if chunk.max.is_none_or(|(_, m)| v > m) {
                chunk.max = Some((t, v));
            }
        }

        chunk
    }

    /// Its points, oldest first. Ordered by time and never by value, so that x stays monotonic and
    /// a negative `scale` at the call site flips the line rather than reversing it.
    fn points(self) -> impl Iterator<Item = (DateTime<Utc>, f64)> {
        let mut points = [
            self.min,
            if self.max == self.min { None } else { self.max },
            self.nan_at.map(|t| (t, f64::NAN)),
        ];
        points.sort_by_key(|point| point.map(|(t, _)| t));

        points.into_iter().flatten()
    }

    /// Collapses `window` to roughly a budget's worth of points, keeping each chunk's extremes
    /// rather than an average so that a spike survives being zoomed out.
    ///
    /// Chunks are anchored to the index of a sample in its whole series rather than to the window,
    /// so their boundaries do not slide as the view pans and a chunk keeps contributing the same
    /// points from frame to frame. `cached` answers for the chunks the window covers whole, as far
    /// as it reaches; the two the window cuts in half are always read through `chunk`. Every sample
    /// still lands in exactly one chunk, so the extremes, and with them the y range the plot picks,
    /// are the same as if nothing had been dropped.
    fn assemble(
        window: Range<usize>,
        stride: usize,
        cached: &[Chunk],
        chunk: &dyn Fn(Range<usize>) -> Chunk,
    ) -> Points {
        let Range { start, end } = window;
        let head_end = (start.div_ceil(stride) * stride).min(end);
        let tail_start = (end / stride * stride).max(head_end);

        let mut points = Vec::new();

        if start < head_end {
            points.extend(chunk(start..head_end).points());
        }
        for i in head_end / stride..tail_start / stride {
            let whole = cached
                .get(i)
                .copied()
                .unwrap_or_else(|| chunk(i * stride..(i + 1) * stride));
            points.extend(whole.points());
        }
        if tail_start < end {
            points.extend(chunk(tail_start..end).points());
        }

        points
    }
}

/// What one plot line asks for.
#[derive(Clone, Copy, Default)]
pub struct TimeseriesArgs<'a> {
    pub system_id: u8,
    pub component_id: u8,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub instance: Option<(&'a str, i64)>,
    /// Stored value meaning "no reading", dropped before anything aggregates it.
    pub sentinel: Option<f64>,
    /// `None` returns every point. `Some(n)` collapses chunks to their extremes to stay near `n`.
    pub max_points: Option<usize>,
}

pub fn format_message_label(name: &str, instance: Option<&MessageInstance>) -> String {
    match instance {
        Some(i) => format!("{name}[{}={}]", i.field, i.value),
        None => name.to_owned(),
    }
}

nadir_macros::implement_message_ext_for_dialect!(
    "common",
    mavspec::rust::dialects::Common,
    mavspec::rust::dialects::common
);
nadir_macros::implement_message_ext_for_dialect!(
    "ardupilotmega",
    mavspec::rust::dialects::Ardupilotmega,
    mavspec::rust::dialects::ardupilotmega
);
nadir_macros::implement_message_ext_for_dialect!(
    "rapid",
    rapid_dialect::Rapid,
    rapid_dialect::rapid
);

#[cfg(test)]
mod tests {
    use super::*;

    use mavspec::rust::dialects::common::messages as common;
    use rapid_dialect::rapid::messages as rapid;

    /// Every point of one system's series, which is what most of these tests want.
    fn args<'a>(system_id: u8, component_id: u8) -> TimeseriesArgs<'a> {
        TimeseriesArgs {
            system_id,
            component_id,
            ..Default::default()
        }
    }

    const BASE_MICROS: i64 = 1_000_000;

    /// Timestamp of the `i`th sample the helpers below write.
    fn at(i: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(BASE_MICROS).unwrap() + chrono::TimeDelta::milliseconds(i)
    }

    /// A store holding `rolls` as `ATTITUDE.roll`, one sample per millisecond.
    fn attitudes(rolls: impl IntoIterator<Item = f32>) -> Db {
        let db = Db::init();

        for (i, roll) in rolls.into_iter().enumerate() {
            db.write_message_at(
                1,
                1,
                &common::Attitude {
                    roll,
                    ..Default::default()
                },
                at(i as i64),
            );
        }

        db
    }

    fn rolls(db: &Db, args: TimeseriesArgs<'_>) -> Vec<(DateTime<Utc>, f64)> {
        db.timeseries_by_name("ATTITUDE", "roll", args).unwrap()
    }

    #[test]
    fn the_window_is_bounded_above_by_until() {
        let db = attitudes((0..10u8).map(f32::from));

        let series = rolls(
            &db,
            TimeseriesArgs {
                since: Some(at(1)),
                until: Some(at(4)),
                ..args(1, 1)
            },
        );

        // Exclusive below, inclusive above.
        let values: Vec<f64> = series.iter().map(|(_, v)| *v).collect();
        assert_eq!(values, [2.0, 3.0, 4.0]);
    }

    #[test]
    fn a_window_within_the_budget_is_returned_verbatim() {
        let db = attitudes((0..100u8).map(f32::from));

        let series = rolls(
            &db,
            TimeseriesArgs {
                max_points: Some(1000),
                ..args(1, 1)
            },
        );

        assert_eq!(series.len(), 100);
        assert_eq!(series[7], (at(7), 7.0));
    }

    #[test]
    fn decimation_keeps_the_extremes_and_their_timestamps() {
        // A single-sample spike, the thing an averaging decimation would erase.
        let db = attitudes((0..10_000).map(|i| if i == 4_321 { 999.0 } else { (i % 7) as f32 }));

        let series = rolls(
            &db,
            TimeseriesArgs {
                max_points: Some(200),
                ..args(1, 1)
            },
        );

        assert!(series.len() < 400, "{} points", series.len());
        assert!(series.contains(&(at(4_321), 999.0)));

        let max = series.iter().map(|(_, v)| *v).fold(f64::MIN, f64::max);
        let min = series.iter().map(|(_, v)| *v).fold(f64::MAX, f64::min);
        assert_eq!((min, max), (0.0, 999.0));

        assert!(series.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn the_chunk_grid_does_not_move_with_the_window() {
        let db = attitudes((0..10_000).map(|i| ((i * 37) % 101) as f32));
        let decimated = |since| {
            rolls(
                &db,
                TimeseriesArgs {
                    since,
                    max_points: Some(200),
                    ..args(1, 1)
                },
            )
        };

        // Two windows whose starts fall in different places inside the same chunk.
        let from_start = decimated(None);
        let shifted = decimated(Some(at(5)));

        let overlap: Vec<_> = from_start
            .iter()
            .filter(|(t, _)| *t > at(1_000))
            .copied()
            .collect();
        let shifted_overlap: Vec<_> = shifted
            .iter()
            .filter(|(t, _)| *t > at(1_000))
            .copied()
            .collect();
        assert_eq!(overlap, shifted_overlap);
    }

    #[test]
    fn a_nan_inside_a_chunk_still_breaks_the_line() {
        let mut values: Vec<f32> = (0..10_000).map(|i| (i % 7) as f32).collect();
        values[4_321] = f32::NAN;
        let db = attitudes(values);

        let series = rolls(
            &db,
            TimeseriesArgs {
                max_points: Some(200),
                ..args(1, 1)
            },
        );

        let nans: Vec<_> = series.iter().filter(|(_, v)| v.is_nan()).collect();
        assert_eq!(nans.len(), 1);
        assert_eq!(nans[0].0, at(4_321));
    }

    #[test]
    fn a_chunk_with_nothing_finite_in_it_emits_no_extremes() {
        let db = attitudes((0..10_000).map(|i| if i < 5_000 { f32::NAN } else { 1.0 }));

        let series = rolls(
            &db,
            TimeseriesArgs {
                max_points: Some(200),
                ..args(1, 1)
            },
        );

        assert!(series.iter().all(|(_, v)| !v.is_infinite()));
        assert!(series.iter().any(|(_, v)| *v == 1.0));
    }

    #[test]
    fn a_sentinel_never_becomes_an_extreme() {
        // The firmware's "no reading" value, larger than any real one.
        let db = attitudes((0..10_000).map(|i| if i % 100 == 0 { 65535.0 } else { 1.0 }));
        let sentinel = |sentinel| {
            rolls(
                &db,
                TimeseriesArgs {
                    sentinel,
                    max_points: Some(200),
                    ..args(1, 1)
                },
            )
        };

        let filtered = sentinel(Some(65535.0));
        assert!(filtered.iter().all(|(_, v)| *v == 1.0));

        // Dropped, not turned into a gap: the line still connects across a dead sensor.
        assert!(filtered.iter().all(|(_, v)| !v.is_nan()));
        assert!(sentinel(None).iter().any(|(_, v)| *v == 65535.0));
    }

    #[test]
    fn a_constant_chunk_emits_one_point_per_chunk() {
        let db = attitudes(std::iter::repeat_n(1.0, 10_000));

        let series = rolls(
            &db,
            TimeseriesArgs {
                max_points: Some(200),
                ..args(1, 1)
            },
        );

        // 10_000 samples at stride 128 is 79 chunks, each contributing its single distinct sample.
        assert_eq!(series.len(), 79);
    }

    #[test]
    fn appending_extends_the_cache_rather_than_invalidating_it() {
        let db = attitudes((0..200_000).map(|i| ((i * 37) % 101) as f32));
        let decimated = || {
            rolls(
                &db,
                TimeseriesArgs {
                    until: Some(at(199_999)),
                    max_points: Some(2_000),
                    ..args(1, 1)
                },
            )
        };

        // Builds the chunks, then extends them over another 50_000 samples. What the first query
        // covered has to come back identical, or a growing series would redraw its own past.
        let before = decimated();
        for i in 200_000..250_000i64 {
            db.write_message_at(
                1,
                1,
                &common::Attitude {
                    roll: ((i * 37) % 101) as f32,
                    ..Default::default()
                },
                at(i),
            );
        }

        assert_eq!(before, decimated());
    }

    #[test]
    fn the_cache_distinguishes_lines_by_their_sentinel() {
        let db = attitudes((0..200_000).map(|i| if i % 1_000 == 0 { 65535.0 } else { 1.0 }));
        let decimated = |sentinel| {
            rolls(
                &db,
                TimeseriesArgs {
                    sentinel,
                    max_points: Some(2_000),
                    ..args(1, 1)
                },
            )
        };

        let filtered = decimated(Some(65535.0));
        let raw = decimated(None);

        assert!(filtered.iter().all(|(_, v)| *v == 1.0));
        assert!(raw.iter().any(|(_, v)| *v == 65535.0));
        assert_eq!(filtered, decimated(Some(65535.0)));
    }

    #[test]
    fn an_empty_window_is_empty_rather_than_an_error() {
        let db = attitudes((0..10u8).map(f32::from));

        let series = rolls(
            &db,
            TimeseriesArgs {
                since: Some(at(100)),
                max_points: Some(200),
                ..args(1, 1)
            },
        );

        assert!(series.is_empty());
    }

    #[test]
    fn the_summary_counts_every_instance_and_dialect_of_a_message() {
        let db = Db::init();

        for (id, pressure1) in [(0_u8, 100_u16), (1, 200), (0, 150)] {
            db.write_message(
                1,
                1,
                &rapid::PressureVessel {
                    id,
                    pressure1,
                    ..Default::default()
                },
            );
        }
        // Both are message 76, stored under two types and reported as one row.
        db.write_message(1, 1, &common::CommandLong::default());
        db.write_message(1, 1, &rapid::CommandLong::default());
        db.write_message(2, 1, &common::Attitude::default());

        let summary = db.message_summary(1, 1);
        let row = |name: &str, instance| {
            summary
                .iter()
                .find(|row| row.name == name && row.instance.as_ref().map(|i| i.value) == instance)
                .map(|row| (row.count, row.freq_hz))
        };

        assert_eq!(row("PRESSURE_VESSEL", Some(0)), Some((2, 0.4)));
        assert_eq!(row("PRESSURE_VESSEL", Some(1)), Some((1, 0.2)));
        assert_eq!(row("COMMAND_LONG", None), Some((2, 0.4)));

        // Another system's messages stay out of it, and the rows are sorted by id then instance.
        assert!(row("ATTITUDE", None).is_none());
        assert!(
            summary
                .windows(2)
                .all(|w| (w[0].msg_id, w[0].instance.as_ref().map(|i| i.value))
                    < (w[1].msg_id, w[1].instance.as_ref().map(|i| i.value)))
        );
    }

    #[test]
    fn a_dialects_own_type_reads_back_only_its_own_messages() {
        let db = Db::init();

        db.write_message(
            1,
            1,
            &common::CommandLong {
                command: mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm,
                ..Default::default()
            },
        );

        db.write_message(
            1,
            1,
            &rapid::CommandLong {
                command: rapid_dialect::rapid::enums::MavCmd::CommandValve,
                ..Default::default()
            },
        );

        // Both are message 76, but they are separate Rust types and so separate series. Callers
        // that want every command read both, as the commands pane does.
        let common_rows = db.all_messages::<common::CommandLong>(1, 1);
        assert_eq!(common_rows.len(), 1);
        assert_eq!(
            common_rows[0].1.command,
            mavspec::rust::dialects::common::enums::MavCmd::ComponentArmDisarm
        );

        let rapid_rows = db.all_messages::<rapid::CommandLong>(1, 1);
        assert_eq!(rapid_rows.len(), 1);
        assert_eq!(
            rapid_rows[0].1.command,
            rapid_dialect::rapid::enums::MavCmd::CommandValve
        );
    }

    #[test]
    fn floats_round_trip_through_the_timeseries_path() {
        let db = Db::init();

        for roll in [0.0_f32, -1.5, f32::MAX, f32::NAN] {
            db.write_message(
                1,
                1,
                &common::Attitude {
                    roll,
                    ..Default::default()
                },
            );
        }

        let series = db
            .timeseries_by_name("ATTITUDE", "roll", args(1, 1))
            .unwrap();

        let values: Vec<f32> = series.iter().map(|(_, v)| *v as f32).collect();
        assert_eq!(values.len(), 4);
        assert_eq!(values[0], 0.0);
        assert_eq!(values[1], -1.5);
        assert_eq!(values[2], f32::MAX);
        // A NaN reading survives rather than becoming a hole in the series.
        assert!(values[3].is_nan());
        assert!(
            db.last_message::<common::Attitude>(1, 1)
                .unwrap()
                .roll
                .is_nan()
        );
    }

    #[test]
    fn array_fields_survive_a_round_trip() {
        let db = Db::init();

        db.write_message(
            1,
            1,
            &common::CanFrame {
                id: 0x42,
                data: [1, 2, 3, 4, 5, 6, 7, 8],
                ..Default::default()
            },
        );

        let rows = db.all_messages::<common::CanFrame>(1, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1.id, 0x42);
        assert_eq!(rows[0].1.data, [1, 2, 3, 4, 5, 6, 7, 8]);

        // An array has no single number to plot, so it yields no points rather than an error.
        let series = db
            .timeseries_by_name("CAN_FRAME", "data", args(1, 1))
            .unwrap();
        assert!(series.is_empty());
    }

    #[test]
    fn instances_of_one_message_are_stored_apart() {
        let db = Db::init();

        let base = DateTime::from_timestamp_micros(1_000_000).unwrap();
        for (i, (id, pressure1)) in [(0_u8, 100_u16), (1, 200), (0, 150)]
            .into_iter()
            .enumerate()
        {
            db.write_message_at(
                1,
                1,
                &rapid::PressureVessel {
                    id,
                    pressure1,
                    ..Default::default()
                },
                base + chrono::TimeDelta::milliseconds(i as i64),
            );
        }

        let field = rapid::PressureVessel::instance_field().unwrap();
        let vessel = |id| {
            db.last_message_filtered::<rapid::PressureVessel>(1, 1, Some((field, id)))
                .unwrap()
                .pressure1
        };

        assert_eq!(vessel(0), 150);
        assert_eq!(vessel(1), 200);

        // Unfiltered, the newest across every instance.
        assert_eq!(
            db.last_message::<rapid::PressureVessel>(1, 1)
                .unwrap()
                .pressure1,
            150
        );

        let pressures = |instance| {
            db.timeseries_by_name(
                "PRESSURE_VESSEL",
                "pressure1",
                TimeseriesArgs {
                    instance,
                    ..args(1, 1)
                },
            )
            .unwrap()
            .into_iter()
            .map(|(_, v)| v)
            .collect::<Vec<_>>()
        };

        // A plot line for one instance sees only that instance; without one, every instance
        // merges in receive order.
        assert_eq!(pressures(Some((field, 0))), [100.0, 150.0]);
        assert_eq!(pressures(None), [100.0, 200.0, 150.0]);
    }

    #[test]
    fn messages_since_excludes_the_cursor_and_honours_the_limit() {
        let db = Db::init();

        let base = DateTime::from_timestamp_micros(1_000_000).unwrap();
        for i in 0..5 {
            db.write_message_at(
                1,
                1,
                &common::Attitude {
                    time_boot_ms: i,
                    ..Default::default()
                },
                base + chrono::TimeDelta::milliseconds(i64::from(i)),
            );
        }

        let boot_ms = |rows: Vec<(DateTime<Utc>, common::Attitude)>| {
            rows.into_iter()
                .map(|(_, m)| m.time_boot_ms)
                .collect::<Vec<_>>()
        };

        assert_eq!(boot_ms(db.all_messages(1, 1)), [0, 1, 2, 3, 4]);

        // Strictly newer than the cursor, so re-polling with the last timestamp yields no repeats.
        let cursor = base + chrono::TimeDelta::milliseconds(2);
        assert_eq!(boot_ms(db.messages_since(1, 1, Some(cursor), None)), [3, 4]);

        assert_eq!(boot_ms(db.messages_since(1, 1, None, Some(2))), [0, 1]);
    }

    #[test]
    fn the_debug_view_follows_the_instance_it_was_asked_for() {
        let db = Db::init();

        for (id, pressure1) in [(0_u8, 100_u16), (1, 200)] {
            db.write_message(
                1,
                1,
                &rapid::PressureVessel {
                    id,
                    pressure1,
                    ..Default::default()
                },
            );
        }

        let debug = |instance| {
            db.last_message_debug_by_name("PRESSURE_VESSEL", 1, 1, instance)
                .map(|found| found.is_some_and(|text| text.contains("pressure1: 200")))
        };

        let field = rapid::PressureVessel::instance_field().unwrap();
        assert!(debug(Some((field, 1))).unwrap());
        assert!(!debug(Some((field, 0))).unwrap());
        // Unfiltered, the newest across every instance.
        assert!(debug(None).unwrap());

        assert!(matches!(
            db.last_message_debug_by_name("ATTITUDE", 1, 1, None),
            Err(DbError::NotFound(_))
        ));
        assert!(matches!(
            db.last_message_debug_by_name("NOT_A_MESSAGE", 1, 1, None),
            Ok(None)
        ));
    }

    #[test]
    fn an_unknown_message_or_field_is_an_error_rather_than_a_panic() {
        let db = Db::init();

        assert!(matches!(
            db.timeseries_by_name("NOT_A_MESSAGE", "roll", args(1, 1)),
            Err(DbError::UnknownField(_))
        ));

        assert!(matches!(
            db.timeseries_by_name("ATTITUDE", "not_a_field", args(1, 1)),
            Err(DbError::UnknownField(_))
        ));
    }
}
