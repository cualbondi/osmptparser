/// Result of the best-effort process that reconstructs a linestring from a
/// way or relation, describing which workarounds (if any) had to be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseStatus {
    /// Parsed cleanly, no workarounds applied.
    Ok,
    /// Ways had to be reordered by proximity to form a single linestring.
    Sorted,
    /// Ways (or the two ends of a ring) had to be joined across a gap within tolerance.
    Joined,
    /// Ways had to be both reordered and joined across gaps.
    JoinedSorted,
    /// Could not be assembled into a single linestring.
    Broken,
}

impl ParseStatus {
    /// numeric status code, kept stable for the JSON output
    pub fn code(&self) -> u64 {
        match self {
            ParseStatus::Ok => 0,
            ParseStatus::Sorted => 101,
            ParseStatus::Joined => 102,
            ParseStatus::JoinedSorted => 103,
            ParseStatus::Broken => 501,
        }
    }

    /// human-readable detail, kept stable for the JSON output
    pub fn detail(&self) -> &'static str {
        match self {
            ParseStatus::Ok => "",
            ParseStatus::Sorted => "Sorted",
            ParseStatus::Joined => "Joined",
            ParseStatus::JoinedSorted => "Joined Sorted",
            ParseStatus::Broken => "Broken",
        }
    }
}
