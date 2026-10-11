//! Map tiles kept on disk, so an area once seen or downloaded still draws without a connection.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use egui::{Rect, pos2};
use walkers::sources::{Attribution, OpenStreetMap, TileSource};
use walkers::{Position, Style, Tile, TileId, TilePiece, Tiles};

use nadir_core::settings::MapboxStyle;

/// OSM's tile policy names a unique User-Agent as a condition of use.
const USER_AGENT: &str = concat!(
    "nadir/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/tudsat-rocket/nadir)"
);
const TIMEOUT: Duration = Duration::from_secs(10);
/// How long a tile that failed to load waits before it is asked for again.
const RETRY_AFTER: Duration = Duration::from_secs(15);
const QUEUE_CAPACITY: usize = 64;

pub fn cache_root() -> Option<PathBuf> {
    directories::ProjectDirs::from("space", "tudsat", "nadir")
        .map(|dirs| dirs.cache_dir().join("tiles"))
}

pub fn disk_usage(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };

    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => disk_usage(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Where one tile source lives on disk, and how it is fetched when the disk copy is missing or old.
#[derive(Clone)]
pub struct Store {
    source: Arc<dyn TileSource + Send + Sync>,
    dir: Option<PathBuf>,
    /// Older copies are fetched again when online, and still used when not.
    max_age: Duration,
    workers: usize,
}

impl Store {
    pub fn osm() -> Self {
        // OSM asks for at least 7 days of caching when the headers are not read.
        Self::new(OpenStreetMap, "osm", Duration::from_hours(7 * 24), 2)
    }

    pub fn mapbox(style: MapboxStyle, access_token: String) -> Self {
        let (source, slug) = super::MapPane::mapbox_source(style, access_token);

        // Mapbox sends `max-age=43200`.
        Self::new(
            source,
            &format!("mapbox-{slug}"),
            Duration::from_hours(12),
            6,
        )
    }

    fn new(
        source: impl TileSource + Send + Sync + 'static,
        slug: &str,
        max_age: Duration,
        workers: usize,
    ) -> Self {
        Self {
            source: Arc::new(source),
            dir: cache_root().map(|root| root.join(slug)),
            max_age,
            workers,
        }
    }

    pub fn tile_size(&self) -> u32 {
        self.source.tile_size()
    }

    pub fn max_zoom(&self) -> u8 {
        self.source.max_zoom()
    }

    fn path(&self, id: TileId) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        Some(
            dir.join(id.zoom.to_string())
                .join(id.x.to_string())
                .join(id.y.to_string()),
        )
    }

    fn fresh(&self, path: &Path) -> bool {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| {
                SystemTime::now()
                    .duration_since(modified)
                    .is_ok_and(|age| age < self.max_age)
            })
    }

    /// The tile's bytes: from disk while fresh, downloaded otherwise, and the stale disk copy if
    /// the download fails.
    fn load(&self, id: TileId) -> Result<Vec<u8>, String> {
        let path = self.path(id);
        if let Some(path) = &path
            && self.fresh(path)
            && let Ok(bytes) = std::fs::read(path)
        {
            return Ok(bytes);
        }

        match self.download(id) {
            Ok(bytes) => {
                if let Some(path) = &path {
                    Self::write(path, &bytes);
                }
                Ok(bytes)
            }
            Err(e) => path.and_then(|path| std::fs::read(path).ok()).ok_or(e),
        }
    }

    fn ensure(&self, id: TileId) -> Result<(), String> {
        let path = self.path(id).ok_or("no cache directory")?;
        if self.fresh(&path) {
            return Ok(());
        }

        let bytes = self.download(id)?;
        Self::write(&path, &bytes);
        Ok(())
    }

    fn download(&self, id: TileId) -> Result<Vec<u8>, String> {
        let mut request = ehttp::Request::get(self.source.tile_url(id));
        request.headers.insert("User-Agent", USER_AGENT);
        request.timeout = Some(TIMEOUT);

        // The URL is left out of errors: Mapbox URLs carry the access token.
        let response = ehttp::fetch_blocking(&request)?;
        if !response.ok {
            return Err(format!(
                "tile {}/{}/{}: HTTP {}",
                id.zoom, id.x, id.y, response.status
            ));
        }

        Ok(response.bytes)
    }

    fn write(path: &Path, bytes: &[u8]) {
        let tmp = path.with_extension("tmp");
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&tmp, bytes))
            .and_then(|()| std::fs::rename(&tmp, path));

        if let Err(e) = result {
            tracing::warn!("Failed to cache {}: {e}", path.display());
        }
    }
}

enum Slot {
    Pending,
    Failed(Instant),
    Ready { tile: Tile, used: u64 },
}

#[derive(Default)]
struct Queue {
    /// Newest last, so the tiles currently on screen are served before ones panned past.
    tiles: Mutex<Vec<TileId>>,
    wake: Condvar,
    closed: AtomicBool,
}

pub struct CachedTiles {
    attribution: Attribution,
    tile_size: u32,
    max_zoom: u8,
    capacity: usize,
    slots: HashMap<TileId, Slot>,
    queue: Arc<Queue>,
    loaded_rx: Receiver<(TileId, Option<Tile>)>,
    ctx: egui::Context,
}

impl CachedTiles {
    pub fn new(store: &Store, ctx: &egui::Context) -> Self {
        let queue = Arc::new(Queue::default());
        let (loaded_tx, loaded_rx) = channel();

        for _ in 0..store.workers {
            let store = store.clone();
            let queue = Arc::clone(&queue);
            let loaded_tx = loaded_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || Self::work(&store, &queue, &loaded_tx, &ctx));
        }

        let tile_size = store.tile_size();
        let capacity = 256 * 256 * 256 / (tile_size * tile_size) as usize;

        Self {
            attribution: store.source.attribution(),
            tile_size,
            max_zoom: store.max_zoom(),
            capacity,
            slots: HashMap::new(),
            queue,
            loaded_rx,
            ctx: ctx.clone(),
        }
    }

    fn work(
        store: &Store,
        queue: &Queue,
        loaded_tx: &Sender<(TileId, Option<Tile>)>,
        ctx: &egui::Context,
    ) {
        loop {
            let id = {
                let Ok(mut tiles) = queue.tiles.lock() else {
                    return;
                };
                loop {
                    if queue.closed.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Some(id) = tiles.pop() {
                        break id;
                    }
                    let Ok(guard) = queue.wake.wait(tiles) else {
                        return;
                    };
                    tiles = guard;
                }
            };

            let tile = store
                .load(id)
                .and_then(|bytes| {
                    Tile::new(&bytes, &Style::default(), id.zoom, ctx).map_err(|e| e.to_string())
                })
                .inspect_err(|e| tracing::debug!("Failed to load map tile: {e}"))
                .ok();

            if loaded_tx.send((id, tile)).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }

    fn receive(&mut self) {
        let now = self.ctx.cumulative_pass_nr();
        while let Ok((id, tile)) = self.loaded_rx.try_recv() {
            let slot = match tile {
                Some(tile) => Slot::Ready { tile, used: now },
                None => Slot::Failed(Instant::now()),
            };
            self.slots.insert(id, slot);
        }

        while self.slots.len() > self.capacity {
            let oldest = self
                .slots
                .iter()
                .filter_map(|(id, slot)| match slot {
                    Slot::Ready { used, .. } => Some((*used, *id)),
                    _ => None,
                })
                .min_by_key(|(used, _)| *used);
            let Some((used, id)) = oldest else { break };
            if used == now {
                break;
            }
            self.slots.remove(&id);
        }
    }

    fn request(&mut self, id: TileId) {
        match self.slots.get(&id) {
            Some(Slot::Ready { .. } | Slot::Pending) => return,
            Some(Slot::Failed(at)) if at.elapsed() < RETRY_AFTER => return,
            _ => {}
        }

        let Ok(mut tiles) = self.queue.tiles.lock() else {
            return;
        };
        if tiles.len() >= QUEUE_CAPACITY {
            let dropped = tiles.remove(0);
            self.slots.remove(&dropped);
        }
        tiles.push(id);
        self.slots.insert(id, Slot::Pending);
        self.queue.wake.notify_one();
    }
}

impl Drop for CachedTiles {
    fn drop(&mut self) {
        self.queue.closed.store(true, Ordering::Relaxed);
        self.queue.wake.notify_all();
    }
}

impl Tiles for CachedTiles {
    fn at(&mut self, tile_id: TileId) -> Option<TilePiece> {
        self.receive();

        let side = 1u32.checked_shl(u32::from(tile_id.zoom))?;
        if tile_id.x >= side || tile_id.y >= side {
            return None;
        }

        self.request(interpolate_from_lower_zoom(tile_id, tile_id.zoom.min(self.max_zoom)).0);

        let now = self.ctx.cumulative_pass_nr();
        (0..=tile_id.zoom.min(self.max_zoom))
            .rev()
            .find_map(|zoom| {
                let (id, uv) = interpolate_from_lower_zoom(tile_id, zoom);
                match self.slots.get_mut(&id) {
                    Some(Slot::Ready { tile, used }) => {
                        *used = now;
                        Some(TilePiece::new(tile.clone(), uv))
                    }
                    _ => None,
                }
            })
    }

    fn attribution(&self) -> Attribution {
        self.attribution.clone()
    }

    fn tile_size(&self) -> u32 {
        self.tile_size
    }
}

/// The tile at `zoom` covering `tile_id`, and the part of it that does. Copied from walkers, which
/// keeps it crate-private.
fn interpolate_from_lower_zoom(tile_id: TileId, zoom: u8) -> (TileId, Rect) {
    let dzoom = 1u32 << (tile_id.zoom - zoom);
    let id = TileId {
        x: tile_id.x / dzoom,
        y: tile_id.y / dzoom,
        zoom,
    };

    let z = (dzoom as f32).recip();
    let (u, v) = (
        (tile_id.x % dzoom) as f32 * z,
        (tile_id.y % dzoom) as f32 * z,
    );

    (id, Rect::from_min_max(pos2(u, v), pos2(u + z, v + z)))
}

/// A circle around a point, down to a map zoom level, as the tiles of a source that cover it.
#[derive(Clone, Copy)]
pub struct Area {
    pub center: Position,
    pub radius_m: f64,
    /// In map zoom levels, which are 256 px tiles.
    pub max_zoom: u8,
}

impl Area {
    fn ranges(self, store: &Store) -> Vec<(u8, RangeInclusive<u32>, RangeInclusive<u32>)> {
        const METERS_PER_DEGREE: f64 = 111_320.0;

        let dlat = self.radius_m / METERS_PER_DEGREE;
        let dlon =
            self.radius_m / (METERS_PER_DEGREE * self.center.y().to_radians().cos().max(0.01));
        let north_west = Position::new(self.center.x() - dlon, self.center.y() + dlat);
        let south_east = Position::new(self.center.x() + dlon, self.center.y() - dlat);

        // A 512 px source tile at zoom z covers what four 256 px tiles at z + 1 do.
        let offset = (store.tile_size() / 256).ilog2() as u8;
        let max_zoom = self.max_zoom.saturating_sub(offset).min(store.max_zoom());

        (0..=max_zoom)
            .map(|zoom| {
                let (x0, y0) = Self::tile_xy(north_west, zoom);
                let (x1, y1) = Self::tile_xy(south_east, zoom);
                (zoom, x0..=x1, y0..=y1)
            })
            .collect()
    }

    fn tile_xy(position: Position, zoom: u8) -> (u32, u32) {
        let (x, y) = super::mercator(position);
        let side = 1u32 << zoom;
        let tile = |v: f64| ((v * f64::from(side)).floor().max(0.0) as u32).min(side - 1);
        (tile(x), tile(y))
    }

    pub fn count(self, store: &Store) -> usize {
        self.ranges(store)
            .into_iter()
            .map(|(_, xs, ys)| xs.count() * ys.count())
            .sum()
    }

    fn ids(self, store: &Store) -> Vec<TileId> {
        self.ranges(store)
            .into_iter()
            .flat_map(|(zoom, xs, ys)| {
                xs.flat_map(move |x| ys.clone().map(move |y| TileId { x, y, zoom }))
            })
            .collect()
    }
}

/// An area being downloaded to disk in the background. Dropping it stops the download.
pub struct Prefetch {
    pub total: usize,
    done: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    last_error: Arc<Mutex<Option<String>>>,
    cancel: Arc<AtomicBool>,
}

impl Prefetch {
    pub fn start(store: &Store, area: Area, ctx: &egui::Context) -> Self {
        let ids = Arc::new(area.ids(store));
        let next = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let last_error = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));

        for _ in 0..store.workers {
            let (store, ids, next) = (store.clone(), Arc::clone(&ids), Arc::clone(&next));
            let (done, failed, cancel) =
                (Arc::clone(&done), Arc::clone(&failed), Arc::clone(&cancel));
            let last_error = Arc::clone(&last_error);
            let ctx = ctx.clone();

            std::thread::spawn(move || {
                while !cancel.load(Ordering::Relaxed) {
                    let Some(id) = ids.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    if let Err(e) = store.ensure(*id) {
                        tracing::debug!("Failed to download map tile: {e}");
                        failed.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut last) = last_error.lock() {
                            *last = Some(e);
                        }
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                    ctx.request_repaint();
                }
            });
        }

        Self {
            total: ids.len(),
            done,
            failed,
            last_error,
            cancel,
        }
    }

    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }

    pub fn failed(&self) -> usize {
        self.failed.load(Ordering::Relaxed)
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok()?.clone()
    }

    pub fn finished(&self) -> bool {
        self.done() >= self.total
    }
}

impl Drop for Prefetch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_picks_the_covering_quarter() {
        let (id, uv) = interpolate_from_lower_zoom(
            TileId {
                x: 3,
                y: 2,
                zoom: 2,
            },
            1,
        );

        assert_eq!(
            id,
            TileId {
                x: 1,
                y: 1,
                zoom: 1
            }
        );
        assert_eq!(uv, Rect::from_min_max(pos2(0.5, 0.0), pos2(1.0, 0.5)));
    }

    #[test]
    fn an_area_counts_one_tile_per_level_until_it_spans_more() {
        let area = Area {
            center: Position::new(8.59, 49.86),
            radius_m: 1000.0,
            max_zoom: 19,
        };
        let osm = Store::osm();
        let ranges = area.ranges(&osm);

        assert_eq!(ranges.len(), 20);
        assert_eq!(ranges[0].1.clone().count() * ranges[0].2.clone().count(), 1);
        assert_eq!(area.count(&osm), area.ids(&osm).len());

        // 2 km across, at ~49.9 deg N a zoom 15 tile is ~790 m wide.
        let (_, xs, ys) = &ranges[15];
        assert!((3..=4).contains(&xs.clone().count()), "{xs:?}");
        assert!((3..=5).contains(&ys.clone().count()), "{ys:?}");
    }

    #[test]
    fn mapbox_tiles_stop_one_level_earlier() {
        let area = Area {
            center: Position::new(8.59, 49.86),
            radius_m: 1000.0,
            max_zoom: 17,
        };

        assert_eq!(
            area.ranges(&Store::mapbox(MapboxStyle::Satellite, String::new()))
                .len(),
            17
        );
    }
}
