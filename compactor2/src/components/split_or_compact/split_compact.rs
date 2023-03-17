use std::fmt::Display;

use data_types::{CompactionLevel, ParquetFile};

use crate::{file_classification::FilesToCompactOrSplit, partition_info::PartitionInfo};

use super::{
    files_to_compact::limit_files_to_compact,
    large_files_to_split::compute_split_times_for_large_files,
    start_level_files_to_split::identify_start_level_files_to_split, SplitOrCompact,
};

#[derive(Debug)]
pub struct SplitCompact {
    max_compact_size: usize,
    max_desired_file_size: u64,
}

impl SplitCompact {
    pub fn new(max_compact_size: usize, max_desired_file_size: u64) -> Self {
        Self {
            max_compact_size,
            max_desired_file_size,
        }
    }
}

impl Display for SplitCompact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "split_or_compact({}, {})",
            self.max_compact_size, self.max_desired_file_size
        )
    }
}

impl SplitOrCompact for SplitCompact {
    /// Return (`[files_to_split_or_compact]`, `[files_to_keep]`) of given files
    ///
    /// Verify if the the give files are over the max_compact_size limit
    /// If so, find start-level files that can be split to reduce the number of overlapped files that must be compact in one run.
    /// If split is not needed, pick files to compact that under max_compact_size limit
    fn apply(
        &self,
        _partition_info: &PartitionInfo,
        files: Vec<ParquetFile>,
        target_level: CompactionLevel,
    ) -> (FilesToCompactOrSplit, Vec<ParquetFile>) {
        // Compact all in one run if total size is less than max_compact_size
        let total_size: i64 = files.iter().map(|f| f.file_size_bytes).sum();
        if total_size as usize <= self.max_compact_size {
            return (FilesToCompactOrSplit::FilesToCompact(files), vec![]);
        }

        // This function identifies all start-level files that overlap with more than one target-level files
        let (files_to_split, files_not_to_split) =
            identify_start_level_files_to_split(files, target_level);

        if !files_to_split.is_empty() {
            // These files must be split before further compaction
            return (
                FilesToCompactOrSplit::FilesToSplit(files_to_split),
                files_not_to_split,
            );
        }

        // No start level split is needed, which means every start-level file overlaps with at most one target-level file
        // Need to limit number of files to compact to stay under compact size limit
        let keep_and_compact_or_split =
            limit_files_to_compact(self.max_compact_size, files_not_to_split, target_level);

        let files_to_compact = keep_and_compact_or_split.files_to_compact();
        let files_to_further_split = keep_and_compact_or_split.files_to_further_split();
        let mut files_to_keep = keep_and_compact_or_split.files_to_keep();

        if !files_to_compact.is_empty() {
            return (
                FilesToCompactOrSplit::FilesToCompact(files_to_compact),
                files_to_keep,
            );
        }

        let (files_to_split, files_not_to_split) = compute_split_times_for_large_files(
            files_to_further_split,
            self.max_desired_file_size,
            self.max_compact_size,
        );

        files_to_keep.extend(files_not_to_split);

        (
            FilesToCompactOrSplit::FilesToSplit(files_to_split),
            files_to_keep,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use compactor2_test_utils::{
        create_overlapped_l0_l1_files_2, create_overlapped_l1_l2_files_2, format_files,
        format_files_split,
    };
    use data_types::CompactionLevel;

    use crate::{
        components::split_or_compact::{
            large_files_to_split::PERCENTAGE_OF_SOFT_EXCEEDED, split_compact::SplitCompact,
            SplitOrCompact,
        },
        test_utils::PartitionInfoBuilder,
    };

    const FILE_SIZE: usize = 100;

    #[test]
    fn test_empty() {
        let files = vec![];
        let p_info = Arc::new(PartitionInfoBuilder::new().build());
        let split_compact = SplitCompact::new(FILE_SIZE, FILE_SIZE as u64);
        let (files_to_compact_or_split, files_to_keep) =
            split_compact.apply(&p_info, files, CompactionLevel::Initial);

        assert!(files_to_compact_or_split.is_empty());
        assert!(files_to_keep.is_empty());
    }

    #[test]
    #[should_panic(
        expected = "max_compact_size 100 must be at least 2 times larger than max_desired_file_size 100"
    )]
    fn test_compact_invalid_max_compact_size() {
        let files = create_overlapped_l1_l2_files_2(FILE_SIZE as i64);
        insta::assert_yaml_snapshot!(
            format_files("initial", &files),
            @r###"
        ---
        - initial
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 4ns                                                                               |-----L1.13------|"
        - "L1.12[400,500] 3ns                                           |-----L1.12------|                                    "
        - "L1.11[250,350] 2ns                |-----L1.11------|                                                               "
        - "L2, all files 100b                                                                                                 "
        - "L2.22[200,300] 1ns       |-----L2.22------|                                                                        "
        "###
        );

        let p_info = Arc::new(PartitionInfoBuilder::new().build());
        let split_compact = SplitCompact::new(FILE_SIZE, FILE_SIZE as u64);
        let (_files_to_compact_or_split, _files_to_keep) =
            split_compact.apply(&p_info, files, CompactionLevel::Final);
    }

    #[test]
    fn test_compact_too_large_to_compact() {
        let files = create_overlapped_l1_l2_files_2(FILE_SIZE as i64);
        insta::assert_yaml_snapshot!(
            format_files("initial", &files),
            @r###"
        ---
        - initial
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 4ns                                                                               |-----L1.13------|"
        - "L1.12[400,500] 3ns                                           |-----L1.12------|                                    "
        - "L1.11[250,350] 2ns                |-----L1.11------|                                                               "
        - "L2, all files 100b                                                                                                 "
        - "L2.22[200,300] 1ns       |-----L2.22------|                                                                        "
        "###
        );

        let p_info = Arc::new(PartitionInfoBuilder::new().build());
        let max_desired_file_size =
            FILE_SIZE - (FILE_SIZE as f64 * PERCENTAGE_OF_SOFT_EXCEEDED) as usize - 30;
        let max_compact_size = 3 * max_desired_file_size;
        let split_compact = SplitCompact::new(max_compact_size, max_desired_file_size as u64);
        let (files_to_compact_or_split, files_to_keep) =
            split_compact.apply(&p_info, files, CompactionLevel::Final);

        // need to split files
        let files_to_compact = files_to_compact_or_split.files_to_compact();
        let files_to_split = files_to_compact_or_split.files_to_split();
        let split_times = files_to_compact_or_split.split_times();
        assert!(files_to_compact.is_empty());
        assert_eq!(files_to_split.len(), 2);
        assert_eq!(files_to_keep.len(), 2);
        // both L1.11 and L2.22 are just a bit larger than max_desired_file_size
        // so they are split into 2 files each. This means the split_times of each includes one time where it is split into 2 files
        for times in split_times {
            assert_eq!(times.len(), 1);
        }

        // See layout of 2 set of files
        insta::assert_yaml_snapshot!(
            format_files_split("files to split", &files_to_compact_or_split.files_to_split() , "files to keep:", &files_to_keep),
            @r###"
        ---
        - files to split
        - "L1, all files 100b                                                                                                 "
        - "L1.11[250,350] 2ns                                     |--------------------------L1.11---------------------------|"
        - "L2, all files 100b                                                                                                 "
        - "L2.22[200,300] 1ns       |--------------------------L2.22---------------------------|                              "
        - "files to keep:"
        - "L1, all files 100b                                                                                                 "
        - "L1.12[400,500] 3ns       |-----------L1.12------------|                                                            "
        - "L1.13[600,700] 4ns                                                                   |-----------L1.13------------|"
        "###
        );
    }

    #[test]
    fn test_compact_files_no_limit() {
        let files = create_overlapped_l0_l1_files_2(FILE_SIZE as i64);
        insta::assert_yaml_snapshot!(
            format_files("initial", &files),
            @r###"
        ---
        - initial
        - "L0, all files 100b                                                                                                 "
        - "L0.2[650,750] 180s                                                    |------L0.2------|                           "
        - "L0.1[450,620] 120s                |------------L0.1------------|                                                   "
        - "L0.3[800,900] 300s                                                                               |------L0.3------|"
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 60s                                           |-----L1.13------|                                    "
        - "L1.12[400,500] 60s       |-----L1.12------|                                                                        "
        "###
        );

        // size limit > total size --> compact all
        let p_info = Arc::new(PartitionInfoBuilder::new().build());
        let split_compact = SplitCompact::new(FILE_SIZE * 6 + 1, FILE_SIZE as u64);
        let (files_to_compact_or_split, files_to_keep) =
            split_compact.apply(&p_info, files, CompactionLevel::FileNonOverlapped);

        assert_eq!(files_to_compact_or_split.files_to_compact_len(), 5);
        assert!(files_to_keep.is_empty());

        // See layout of 2 set of files
        insta::assert_yaml_snapshot!(
            format_files_split("files to compact", &files_to_compact_or_split.files_to_compact() , "files to keep:", &files_to_keep),
            @r###"
        ---
        - files to compact
        - "L0, all files 100b                                                                                                 "
        - "L0.2[650,750] 180s                                                    |------L0.2------|                           "
        - "L0.1[450,620] 120s                |------------L0.1------------|                                                   "
        - "L0.3[800,900] 300s                                                                               |------L0.3------|"
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 60s                                           |-----L1.13------|                                    "
        - "L1.12[400,500] 60s       |-----L1.12------|                                                                        "
        - "files to keep:"
        "###
        );
    }

    #[test]
    fn test_split_files() {
        let files = create_overlapped_l0_l1_files_2(FILE_SIZE as i64);
        insta::assert_yaml_snapshot!(
            format_files("initial", &files),
            @r###"
        ---
        - initial
        - "L0, all files 100b                                                                                                 "
        - "L0.2[650,750] 180s                                                    |------L0.2------|                           "
        - "L0.1[450,620] 120s                |------------L0.1------------|                                                   "
        - "L0.3[800,900] 300s                                                                               |------L0.3------|"
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 60s                                           |-----L1.13------|                                    "
        - "L1.12[400,500] 60s       |-----L1.12------|                                                                        "
        "###
        );

        // hit size limit -> split start_level files that overlap with more than 1 target_level files
        let p_info = Arc::new(PartitionInfoBuilder::new().build());
        let split_compact = SplitCompact::new(FILE_SIZE, FILE_SIZE as u64);
        let (files_to_compact_or_split, files_to_keep) =
            split_compact.apply(&p_info, files, CompactionLevel::FileNonOverlapped);

        assert_eq!(files_to_compact_or_split.files_to_split_len(), 1);
        assert_eq!(files_to_keep.len(), 4);

        // See layout of 2 set of files
        insta::assert_yaml_snapshot!(
            format_files_split("files to compact or split:", &files_to_compact_or_split.files_to_split(), "files to keep:", &files_to_keep),
            @r###"
        ---
        - "files to compact or split:"
        - "L0, all files 100b                                                                                                 "
        - "L0.1[450,620] 120s       |------------------------------------------L0.1------------------------------------------|"
        - "files to keep:"
        - "L0, all files 100b                                                                                                 "
        - "L0.2[650,750] 180s                                                    |------L0.2------|                           "
        - "L0.3[800,900] 300s                                                                               |------L0.3------|"
        - "L1, all files 100b                                                                                                 "
        - "L1.12[400,500] 60s       |-----L1.12------|                                                                        "
        - "L1.13[600,700] 60s                                           |-----L1.13------|                                    "
        "###
        );
    }

    #[test]
    fn test_compact_files() {
        let files = create_overlapped_l1_l2_files_2(FILE_SIZE as i64);
        insta::assert_yaml_snapshot!(
            format_files("initial", &files),
            @r###"
        ---
        - initial
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 4ns                                                                               |-----L1.13------|"
        - "L1.12[400,500] 3ns                                           |-----L1.12------|                                    "
        - "L1.11[250,350] 2ns                |-----L1.11------|                                                               "
        - "L2, all files 100b                                                                                                 "
        - "L2.22[200,300] 1ns       |-----L2.22------|                                                                        "
        "###
        );

        // hit size limit and nthign to split --> limit number if files to compact
        let p_info = Arc::new(PartitionInfoBuilder::new().build());
        let split_compact = SplitCompact::new(FILE_SIZE * 3, FILE_SIZE as u64);
        let (files_to_compact_or_split, files_to_keep) =
            split_compact.apply(&p_info, files, CompactionLevel::Final);

        assert_eq!(files_to_compact_or_split.files_to_compact_len(), 3);
        assert_eq!(files_to_keep.len(), 1);

        // See layout of 2 set of files
        insta::assert_yaml_snapshot!(
            format_files_split("files to compact or split:", &files_to_compact_or_split.files_to_compact() , "files to keep:", &files_to_keep),
            @r###"
        ---
        - "files to compact or split:"
        - "L1, all files 100b                                                                                                 "
        - "L1.11[250,350] 2ns                      |-----------L1.11------------|                                             "
        - "L1.12[400,500] 3ns                                                                   |-----------L1.12------------|"
        - "L2, all files 100b                                                                                                 "
        - "L2.22[200,300] 1ns       |-----------L2.22------------|                                                            "
        - "files to keep:"
        - "L1, all files 100b                                                                                                 "
        - "L1.13[600,700] 4ns       |-----------------------------------------L1.13------------------------------------------|"
        "###
        );
    }
}