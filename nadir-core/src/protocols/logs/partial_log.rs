use std::ops::Range;
use tracing::warn;

// #[derive(Debug, Default)]
// pub struct PartialLog {
//     pub data: Vec<u8>,
//     /// A list of missing data ranges, sorted (ascending) by range.start as key.
//     /// Ranges are always merged to take the least amount of entries.
//     /// => Ranges never overlap and never touch.
//     pub missing: BTreeSet<(u32, u32)>,
//     /// number of bytes declared as missing
//     pub missing_size: u32,
// }

// impl PartialLog {
//     fn insert_missing(&mut self, new: Range<u32>) {
//         if new.is_empty() {
//             return;
//         }
//         let backw = self.missing.range((0, 0)..=(new.end, new.end)).rev();
//
//         // let backw = self.missing.range((0..0)..=(new.end..new.end)).rev();
//         if let Some(e) = backw.next() {
//             if e.0 == new.start {
//                 self.missing.remove(e);
//                 self.missing.insert((new.start, new.end));
//                 // merge_touching
//                 todo!()
//             } else {
//                 assert!(new.start < e.start);
//                 // new: |---------|
//                 // e:      |---|
//                 // new: |---------|
//                 // e:      |----------|
//             }
//         }
//
//         match self.missing.binary_search_by_key(&new.start, |r| r.start) {
//             Ok(i) => {
//                 // new.start == missing[i].start
//                 // -> merge new & missing[i]
//                 self.missing[i].end = new.end;
//                 // -> check disjoint condition with missing[i], missing[i++]
//                 Self::merge_touching(&mut self.missing, i);
//             }
//             Err(i) => {
//                 // TODO:
//                 // there is a correctness risk here, which will be fixed
//                 //
//                 // new.start < missing[i].start
//                 // -> merge with left neighbor (if exists)
//                 self.missing.insert(i, new.clone());
//                 // -> check disjoint condition with left neighbor, neighbor++
//                 let i1 = if i > 0 { i - 1 } else { 0 };
//                 Self::merge_touching(&mut self.missing, i1);
//             }
//         };
//         // sanity checks
//         let is_sorted = self.missing.is_sorted_by_key(|r| r.start);
//         assert!(is_sorted);
//         let any_is_touching = self.missing.windows(2).any(|w| w[0].end >= w[1].start);
//         assert!(!any_is_touching);
//     }
// }

// TODO: implement this using BTreeMap
/// Contains a partial binary log upto ``data.len()`` bytes.
/// Missing sections have been filled with placeholder bytes,
/// but are marked in [`PartialLog::missing`].
/// If [`PartialLog::missing`] is empty, one can assume that all bytes in [`PartialLog::data`] are
/// valid.
#[derive(Debug, Default)]
pub struct PartialLogSlow {
    data: Vec<u8>,
    /// A list of missing data ranges, sorted (ascending) by range.start as key.
    /// Ranges are always merged to take the least amount of entries.
    /// => Ranges never overlap and never touch.
    missing: Vec<Range<u32>>,
    /// number of bytes declared as missing
    missing_size: u32,
}
#[derive(Debug, Clone)]
pub enum PartialLogCompleteness {
    Contiguous {
        size: u32,
    },
    NonContigous {
        size: u32,
        downloaded_bytes: u32,
        missing_bytes: u32,
        missing_chunks: u32,
    },
}
impl PartialLogCompleteness {
    /// Returns the lenth of the underlying data vec.
    pub fn len(&self) -> u32 {
        match self {
            Self::Contiguous { size } => *size,
            Self::NonContigous { size, .. } => *size,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn num_valid_byes(&self) -> u32 {
        match self {
            Self::Contiguous { size } => *size,
            Self::NonContigous {
                downloaded_bytes, ..
            } => *downloaded_bytes,
        }
    }
}

impl Default for PartialLogCompleteness {
    fn default() -> Self {
        Self::Contiguous { size: 0 }
    }
}
impl PartialLogSlow {
    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            missing: Vec::new(),
            missing_size: 0,
        }
    }
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            missing: Vec::new(),
            missing_size: 0,
        }
    }
    pub fn get_completeness(&self) -> PartialLogCompleteness {
        if !self.missing.is_empty() {
            assert!(self.missing_size != 0);
        }
        if self.missing.is_empty() {
            PartialLogCompleteness::Contiguous {
                size: self.data.len() as u32,
            }
        } else {
            PartialLogCompleteness::NonContigous {
                size: self.data.len() as u32,
                downloaded_bytes: self.data.len() as u32 - self.missing_size,
                missing_bytes: self.missing_size,
                missing_chunks: self.missing.len() as u32,
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty() && self.missing.is_empty()
    }
    pub fn missing_chunks(&self) -> &Vec<Range<u32>> {
        &self.missing
    }
    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

impl PartialLogSlow {
    /// Ingests arbitrary legth chunks of data with an offset into [`PartialLog::data`].
    /// [`PartialLog::data`] stores the log in order, so it will have at least the size of
    /// ``largest_offset + chunk.len()``.
    /// Parts of chunks that are already stored, will be overwritten when ingested again.
    pub fn ingest(&mut self, offset: u32, chunk: &[u8]) {
        assert_eq!(
            self.missing
                .iter()
                .fold(0, |acc, r| acc + (r.end - r.start)),
            self.missing_size
        );
        if chunk.is_empty() {
            return;
        }
        #[allow(clippy::comparison_chain)]
        if offset as usize == self.data.len() {
            // trivial in order case
            self.data.extend(chunk);
            // tracing::info!("---i chunk inorder");
        } else if offset as usize > self.data.len() {
            // tracing::info!("---m earlier chunk missed");
            // earlier packet has been missed
            self.insert_missing(self.data.len() as u32..offset);
            // -> fill vec with padding
            self.data.resize(offset as usize, 0x0);
            self.data.extend(chunk);
        } else {
            // tracing::info!("---o chunk received out of order");
            // out of order packet received
            // offset < self.data.len()
            //
            if offset as usize + chunk.len() > self.data.len() {
                // eventhough the start of our chunk is already within our vec,
                // the end could still be fresh data
                //                offset
                //                   v
                // chunk:            |-----------|
                // data:   |----------------|
                // brand_new_chunk:          |---|
                // redisc chunk:     |------|
                let brand_new_chunk = &chunk[(self.data.len() - offset as usize)..];
                if !brand_new_chunk.is_empty() {
                    // packet is only partially out of order, handle only out of order part later
                    self.ingest(self.data.len() as u32, brand_new_chunk);
                }
            }
            // chunk:            |---|
            // data:   |----------------|
            // redisc. chunk:    |---|

            let rediscovered_chunk = &chunk[..(self.data.len() - offset as usize).min(chunk.len())];
            // insert rediscovered_chunk into data:
            (self.data.as_mut_slice()
                [offset as usize..(offset as usize + rediscovered_chunk.len())])
                .copy_from_slice(rediscovered_chunk);
            self.eliminate_missing(offset..(offset + rediscovered_chunk.len() as u32));
        }
    }
    fn eliminate_missing(&mut self, to_remove: Range<u32>) {
        if to_remove.is_empty() {
            return;
        }

        let mut found = to_remove;

        let mut i = 0;
        while i < self.missing.len() {
            let missing = self.missing[i].clone();
            assert!(!missing.is_empty());

            let overlap_start = u32::max(found.start, missing.start);
            let overlap_end = u32::min(found.end, missing.end);

            if overlap_start >= overlap_end {
                i += 1;
                continue;
            }

            let new_missing_right = found.end..missing.end;
            let new_missing_left = missing.start..found.start;
            let new_found_left = found.start..missing.start;
            let new_found_right = missing.end..found.end;

            if new_found_right.is_empty() && new_found_left.is_empty() {
                // yay, the found chunk fit completely inside the missing chunk
                // found:       |----|
                // missing: |-----------|
                // -> there are up to 2 chunks that may still be missing
                // -> insert those into list
                // -> finished
                match (!new_missing_left.is_empty(), !new_missing_right.is_empty()) {
                    (true, true) => {
                        self.missing[i] = new_missing_right;
                        self.missing.insert(i, new_missing_left);
                    }
                    (true, false) => self.missing[i] = new_missing_left,
                    (false, true) => self.missing[i] = new_missing_right,
                    (false, false) => {
                        self.missing.remove(i);
                    }
                }
                self.missing_size -= found.end - found.start;
                return;
            }
            if new_found_left.is_empty() {
                assert!(new_missing_right.is_empty());
                if new_missing_left.is_empty() {
                    // found:   |-------|
                    // missing: |----|

                    // -> remove missing chunk
                    self.missing.remove(i);
                    self.missing_size -= missing.end - missing.start;

                    // got to next missing
                } else {
                    // found:       |-------|
                    // missing: |--------|
                    self.missing[i] = new_missing_left.clone();
                    self.missing_size -= missing.end - found.start;
                }
                found = new_found_right;
                i += 1;
                continue;
            }
            if new_found_right.is_empty() {
                if new_missing_right.is_empty() {
                    assert!(!new_missing_left.is_empty());
                    // found:     |------|
                    // missing:     |----|
                    self.missing.remove(i);
                    self.missing_size -= missing.end - missing.start;
                    // because of right to left search, new found chunk can be discarded, because
                    // if there was a missing chunk to intersect with, it would have happened in
                    // the previous iteration
                } else {
                    // found:   |-----|
                    // missing:   |-----|
                    // -> update missing chunk
                    self.missing[i] = new_missing_right.clone();
                    self.missing_size -= found.end - missing.start;
                }
                warn!("part of chunk not found in missing");
                return;
            }

            assert!(new_missing_right.is_empty() && new_missing_left.is_empty());

            // found:  |------------|
            // missing:     |----|
            // -> remove missing chunk
            self.missing.remove(i);
            self.missing_size -= missing.end - missing.start;
            warn!("part of chunk not found in missing");
            // go to next missing chunk
            found = new_found_right;
        }
    }
    fn insert_missing(&mut self, new: Range<u32>) {
        if new.is_empty() {
            return;
        }
        match self.missing.binary_search_by_key(&new.start, |r| r.start) {
            Ok(i) => {
                // new.start == missing[i].start
                // -> merge new & missing[i]
                assert!(self.missing[i].end < new.end);
                self.missing_size += new.end - self.missing[i].end;
                self.missing[i].end = new.end;
                // -> check disjoint condition with missing[i], missing[i++]
                self.missing_size -= Self::merge_touching(&mut self.missing, i);
            }
            Err(i) => {
                if let Some(x) = self.missing.get(i) {
                    assert!(new.start > x.start);
                }
                //
                // -> merge with left neighbor (if exists)
                self.missing.insert(i, new.clone());
                self.missing_size += new.end - new.start;
                // -> check disjoint condition with left neighbor, neighbor++
                let i1 = if i > 0 { i - 1 } else { 0 };
                self.missing_size -= Self::merge_touching(&mut self.missing, i1);
            }
        }
        // sanity checks
        // let is_sorted = self.missing.is_sorted_by_key(|r| r.start);
        // let is_sorted_ends = self.missing.is_sorted_by_key(|r| r.end);
        // assert!(is_sorted);
        // assert!(is_sorted_ends);
        // let any_is_touching = self.missing.windows(2).any(|w| w[0].end >= w[1].start);
        // assert!(!any_is_touching);
    }

    // Merge touching and overlaping neighboring ranges in vec starting from range at index i.
    // Assumes sorted_ranges is sorted (ascending) by range.start as key.
    // Returns number of pruned elements.
    fn merge_touching(sorted_ranges: &mut Vec<Range<u32>>, i: usize) -> u32 {
        let r = sorted_ranges;
        let mut n_pruned = 0;
        while r.get(i + 1).is_some() {
            if r[i].end >= r[i + 1].start {
                let merge_start = r[i].start;
                let merge_end = r[i].end.max(r[i + 1].end);
                n_pruned += r[i].end - r[i + 1].start;
                r[i] = merge_start..merge_end;
                r.remove(i + 1);
            } else {
                return n_pruned;
            }
        }
        n_pruned
    }
}

// Tests for PartialLog generated by Claude Code.
#[cfg(test)]
mod tests {
    use super::PartialLogSlow;

    /// Assert that `log.data == expected` and that `log.missing` is empty.
    fn assert_complete(log: &PartialLogSlow, expected: &[u8]) {
        assert_eq!(
            log.data, expected,
            "data mismatch:\n  got:      {:?}\n  expected: {:?}",
            log.data, expected
        );
        assert!(
            log.missing.is_empty(),
            "missing list should be empty but is: {:?}",
            log.missing
        );
    }

    // -----------------------------------------------------------------------
    // Unit tests – hand-crafted scenarios
    // -----------------------------------------------------------------------

    #[test]
    fn test_in_order_single_chunk() {
        let mut log = PartialLogSlow::new_with_capacity(16);
        let data = b"Hello, world!";
        log.ingest(0, data);
        assert_complete(&log, data);
    }

    #[test]
    fn test_in_order_multiple_chunks() {
        let mut log = PartialLogSlow::new_with_capacity(16);
        log.ingest(0, b"Hello");
        log.ingest(5, b", ");
        log.ingest(7, b"world!");
        assert_complete(&log, b"Hello, world!");
    }

    #[test]
    fn test_single_gap_filled_later() {
        // Send bytes 5-9 first (gap at 0-4), then fill the gap.
        let mut log = PartialLogSlow::new_with_capacity(16);
        log.ingest(5, b"world"); // creates gap 0..5
        log.ingest(0, b"Hello"); // fills gap
        assert_complete(&log, b"Helloworld");
    }

    #[test]
    fn test_gap_exactly_filled() {
        let expected: Vec<u8> = (0u8..20).collect();
        let mut log = PartialLogSlow::new_with_capacity(20);
        log.ingest(10, &expected[10..20]); // gap 0..10
        log.ingest(0, &expected[0..10]); // fills gap exactly
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_multiple_gaps_filled_in_reverse() {
        // Create three separated gaps, fill them all in reverse order.
        let expected: Vec<u8> = (0u8..30).collect();
        let mut log = PartialLogSlow::new_with_capacity(30);
        log.ingest(20, &expected[20..30]); // gap 0..20
        log.ingest(10, &expected[10..20]); // gap 0..10 remains
        log.ingest(0, &expected[0..10]); // fills last gap
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_out_of_order_overlapping_chunk() {
        // chunk [3..10] arrives, then chunk [0..7] arrives (overlaps 3-6 already known)
        let expected: Vec<u8> = (0u8..10).collect();
        let mut log = PartialLogSlow::new_with_capacity(10);
        log.ingest(3, &expected[3..10]); // gap 0..3
        log.ingest(0, &expected[0..7]); // fills gap + overlap
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_chunk_entirely_duplicate() {
        // Same chunk sent twice – second ingest should be a no-op w.r.t. data.
        let expected: Vec<u8> = (0u8..10).collect();
        let mut log = PartialLogSlow::new_with_capacity(10);
        log.ingest(0, &expected);
        log.ingest(0, &expected); // duplicate, fully within known range
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_chunk_partially_duplicate_extending_right() {
        // First 5 bytes known, chunk re-sends them plus 5 new ones.
        let expected: Vec<u8> = (0u8..10).collect();
        let mut log = PartialLogSlow::new_with_capacity(10);
        log.ingest(0, &expected[0..5]);
        log.ingest(0, &expected[0..10]); // starts inside known region
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_gap_then_overlapping_fill() {
        // Bytes 0-4 missing, receive chunk that covers 3-12 (overlaps known + fills part of gap)
        let expected: Vec<u8> = (10u8..23).collect(); // 13 bytes
        let mut log = PartialLogSlow::new_with_capacity(13);
        log.ingest(5, &expected[5..13]); // gap 0..5
        log.ingest(3, &expected[3..10]); // partially fills gap, overlaps known
        log.ingest(0, &expected[0..3]); // fills remaining gap
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_empty_chunk_ignored() {
        let mut log = PartialLogSlow::new_with_capacity(8);
        log.ingest(0, b"");
        log.ingest(4, b"");
        assert_eq!(log.data.len(), 0);
        assert!(log.missing.is_empty());
    }

    #[test]
    fn test_single_byte_chunks_out_of_order() {
        let expected: Vec<u8> = (0u8..8).collect();
        let mut log = PartialLogSlow::new_with_capacity(8);
        // Send in reverse order
        for i in (0..8usize).rev() {
            log.ingest(i as u32, &expected[i..=i]);
        }
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_interleaved_gaps_filled_from_middle() {
        // Pattern: send even-offset bytes, then odd-offset bytes.
        let expected: Vec<u8> = (0u8..16).collect();
        let mut log = PartialLogSlow::new_with_capacity(16);
        for i in (0..16usize).step_by(2) {
            log.ingest(i as u32, &expected[i..=i]);
        }
        for i in (1..16usize).step_by(2) {
            log.ingest(i as u32, &expected[i..=i]);
        }
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_missing_list_merges_adjacent_gaps() {
        // Send byte 0, byte 2, byte 4 – leaving gaps at 1, 3; then fill them.
        let expected: Vec<u8> = (0u8..5).collect();
        let mut log = PartialLogSlow::new_with_capacity(5);
        log.ingest(0, &expected[0..1]);
        log.ingest(2, &expected[2..3]);
        log.ingest(4, &expected[4..5]);
        // missing should be [1..2, 3..4]
        assert_eq!(log.missing, vec![1u32..2, 3..4]);
        log.ingest(1, &expected[1..2]);
        log.ingest(3, &expected[3..4]);
        assert_complete(&log, &expected);
    }

    #[test]
    fn test_large_single_gap_at_start() {
        let expected: Vec<u8> = (0u8..100).collect();
        let mut log = PartialLogSlow::new_with_capacity(100);
        log.ingest(50, &expected[50..100]);
        assert_eq!(log.missing, vec![0u32..50]);
        log.ingest(0, &expected[0..50]);
        assert_complete(&log, &expected);
    }

    // -----------------------------------------------------------------------
    // Property-based / randomised tests
    // -----------------------------------------------------------------------

    /// Simple deterministic LCG so we don't need a dep on `rand`.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0
        }
        fn next_range(&mut self, lo: usize, hi: usize) -> usize {
            lo + (self.next() as usize % (hi - lo))
        }
    }

    /// Core fuzzing helper.
    ///
    /// Generates `ground_truth` of `total_len` bytes, then sends it as randomly
    /// sized chunks (1–90 bytes) in random order, re-sending some chunks to
    /// exercise overlap paths. Finally asserts data matches ground truth.
    fn run_random_test(seed: u64, total_len: usize, resend_probability_pct: u64) {
        let mut rng = Lcg(seed);

        // Build ground truth
        let ground_truth: Vec<u8> = (0..total_len).map(|_| (rng.next() & 0xFF) as u8).collect();

        // Build a list of (offset, chunk) pairs covering the whole range
        let mut packets: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut pos = 0usize;
        while pos < total_len {
            let size = rng.next_range(1, 91).min(total_len - pos);
            packets.push((pos as u32, ground_truth[pos..pos + size].to_vec()));
            pos += size;
        }

        // Optionally duplicate some packets (to exercise overlap / duplicate paths)
        let extra: Vec<(u32, Vec<u8>)> = packets
            .iter()
            .filter(|_| rng.next() % 100 < resend_probability_pct)
            .cloned()
            .collect();
        packets.extend(extra);

        // Shuffle packets with Fisher-Yates
        let n = packets.len();
        for i in (1..n).rev() {
            let j = rng.next_range(0, i + 1);
            packets.swap(i, j);
        }

        // Ingest all packets
        let mut log = PartialLogSlow::new_with_capacity(total_len);
        for (offset, chunk) in &packets {
            log.ingest(*offset, chunk);
        }

        // After all packets the data should be complete and correct
        assert_complete(&log, &ground_truth);
    }

    // Many small logs – exercises lots of boundary conditions at small sizes
    #[test]
    fn fuzz_small_logs() {
        for seed in 0..200 {
            run_random_test(seed, 90, 0);
        }
    }

    // Medium logs, no resends
    #[test]
    fn fuzz_medium_no_resend() {
        for seed in 0..50 {
            run_random_test(seed * 7 + 1, 1024, 0);
        }
    }

    // Medium logs, 30 % resend rate – exercises duplicate / overlap paths heavily
    #[test]
    fn fuzz_medium_with_resends() {
        for seed in 0..50 {
            run_random_test(seed * 13 + 3, 1024, 30);
        }
    }

    // Large log, high resend rate
    #[test]
    fn fuzz_large_high_resend() {
        for seed in 0..10 {
            run_random_test(seed * 31 + 99, 8192, 50);
        }
    }

    // Worst-case: 1-byte packets, maximises number of gaps and merges
    #[test]
    fn fuzz_one_byte_packets() {
        let total_len = 256;
        let mut rng = Lcg(0xDEAD_BEEF);
        let ground_truth: Vec<u8> = (0..total_len).map(|i| i as u8).collect();
        let mut indices: Vec<usize> = (0..total_len).collect();
        // shuffle
        for i in (1..total_len).rev() {
            let j = rng.next_range(0, i + 1);
            indices.swap(i, j);
        }
        let mut log = PartialLogSlow::new_with_capacity(total_len);
        for i in indices {
            log.ingest(i as u32, &ground_truth[i..=i]);
        }
        assert_complete(&log, &ground_truth);
    }

    // Worst-case: maximum packet size (90 bytes) throughout
    #[test]
    fn fuzz_max_packet_size() {
        for seed in 0..20 {
            let mut rng = Lcg(seed * 17 + 5);
            let total_len = 90 * 20; // 1800 bytes, exactly divisible by 90
            let ground_truth: Vec<u8> = (0..total_len).map(|i| i as u8).collect();
            let mut packets: Vec<(u32, Vec<u8>)> = (0..total_len)
                .step_by(90)
                .map(|pos| (pos as u32, ground_truth[pos..pos + 90].to_vec()))
                .collect();
            // shuffle
            let n = packets.len();
            for i in (1..n).rev() {
                let j = rng.next_range(0, i + 1);
                packets.swap(i, j);
            }
            let mut log = PartialLogSlow::new_with_capacity(total_len);
            for (offset, chunk) in &packets {
                log.ingest(*offset, chunk);
            }
            assert_complete(&log, &ground_truth);
        }
    }

    // Stress: many seeds, varying sizes 1..=512, with resends
    #[test]
    fn fuzz_stress() {
        let mut rng = Lcg(0xCAFE_F00D);
        for _ in 0..100 {
            let seed = rng.next();
            let total_len = rng.next_range(1, 5013);
            let resend_pct = rng.next() % 60;
            run_random_test(seed, total_len, resend_pct);
        }
    }
}
