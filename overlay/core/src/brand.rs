//! The product name as it appears on the player's account (rune pages, item sets) and the name
//! those objects carried before the project was renamed. Both are recognised as ours so a page or
//! set made by an older build is still reused and never treated as a personal page.

/// Prefix of every rune page and item set this overlay writes.
pub const NAME: &str = "Recall";
/// The project's previous name; objects with this prefix are ours too.
pub const LEGACY_NAME: &str = "Featherstorm";

/// `Recall`, `Recall Xayah ADC`, `Featherstorm Xayah` are ours; `Recalling` and personal pages are not.
pub fn owns(name: &str) -> bool {
    [NAME, LEGACY_NAME].iter().any(|prefix| {
        name == *prefix
            || name
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(' '))
    })
}

/// `Recall Xayah ADC` / `Recall Xayah`: the rune page and item set name for one loadout.
pub fn loadout_name(champion: &str, position: Option<&str>) -> String {
    match position {
        Some(position) => format!("{NAME} {champion} {position}"),
        None => format!("{NAME} {champion}"),
    }
}

/// The name an older build would have given the same loadout, so it can be replaced.
pub fn legacy_name(name: &str) -> Option<String> {
    name.strip_prefix(NAME)
        .map(|rest| format!("{LEGACY_NAME}{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_both_prefixes_and_nothing_else() {
        for ours in [
            "Recall",
            "Recall Xayah ADC",
            "Featherstorm",
            "Featherstorm Ahri MID",
        ] {
            assert!(owns(ours), "{ours}");
        }
        for personal in [
            "Recalling",
            "Featherstorming",
            "My Recall page",
            "",
            "Xayah",
        ] {
            assert!(!owns(personal), "{personal}");
        }
    }

    #[test]
    fn loadout_names_and_their_legacy_forms() {
        assert_eq!(loadout_name("Xayah", Some("ADC")), "Recall Xayah ADC");
        assert_eq!(loadout_name("Xayah", None), "Recall Xayah");
        assert_eq!(
            legacy_name("Recall Xayah ADC").as_deref(),
            Some("Featherstorm Xayah ADC")
        );
        assert_eq!(legacy_name("Xayah ADC"), None);
    }
}
