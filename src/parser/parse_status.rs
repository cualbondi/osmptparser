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

    /// whether reordering was one of the workarounds applied
    fn sorted(&self) -> bool {
        matches!(self, ParseStatus::Sorted | ParseStatus::JoinedSorted)
    }

    /// whether gap-joining was one of the workarounds applied
    fn joined(&self) -> bool {
        matches!(self, ParseStatus::Joined | ParseStatus::JoinedSorted)
    }

    /// combine the statuses of two consecutive steps (e.g. flatten then close),
    /// keeping every workaround that either step had to apply. `Broken` is
    /// absorbing: if either step failed, the combined result is `Broken`.
    pub fn combine(self, other: ParseStatus) -> ParseStatus {
        if self == ParseStatus::Broken || other == ParseStatus::Broken {
            return ParseStatus::Broken;
        }
        match (
            self.sorted() || other.sorted(),
            self.joined() || other.joined(),
        ) {
            (false, false) => ParseStatus::Ok,
            (true, false) => ParseStatus::Sorted,
            (false, true) => ParseStatus::Joined,
            (true, true) => ParseStatus::JoinedSorted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_keeps_the_flatten_workaround_when_closing_is_clean() {
        // a relation rescued by sorting, whose linestring is already a closed ring,
        // must still report Sorted instead of being downgraded to Ok
        assert_eq!(
            ParseStatus::Sorted.combine(ParseStatus::Ok),
            ParseStatus::Sorted
        );
    }

    #[test]
    fn combine_unions_sorted_and_joined() {
        assert_eq!(
            ParseStatus::Sorted.combine(ParseStatus::Joined),
            ParseStatus::JoinedSorted
        );
    }

    #[test]
    fn combine_is_broken_absorbing() {
        assert_eq!(
            ParseStatus::Sorted.combine(ParseStatus::Broken),
            ParseStatus::Broken
        );
        assert_eq!(
            ParseStatus::Broken.combine(ParseStatus::Ok),
            ParseStatus::Broken
        );
    }

    #[test]
    fn combine_ok_ok_is_ok() {
        assert_eq!(ParseStatus::Ok.combine(ParseStatus::Ok), ParseStatus::Ok);
    }
}
