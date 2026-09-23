//! Laws of version constraints, checked over generated versions instead of a
//! handful of examples: ADR-0004 (a bare version is an exact pin), the semver
//! range rules that BUG-2..5 fixed, the canonical-string round trip the lock
//! file relies on, and newest-first ordering.

use super::*;
use proptest::prelude::*;
use std::cmp::Ordering;

fn prerelease() -> impl Strategy<Value = String> {
    "(alpha|beta|rc)(\\.[1-9][0-9]?)?"
}

fn release() -> impl Strategy<Value = Version> {
    (0u64..6, 0u64..6, 0u64..6).prop_map(|(major, minor, patch)| Version::new(major, minor, patch))
}

fn version() -> impl Strategy<Value = Version> {
    (release(), prop::option::of(prerelease())).prop_map(|(mut version, pre)| {
        if let Some(pre) = pre {
            version.pre = semver::Prerelease::new(&pre).unwrap();
        }
        version
    })
}

/// A pinned version and a candidate that is often the pin itself.
fn pin_and_candidate() -> impl Strategy<Value = (Version, Version)> {
    version().prop_flat_map(|pin| (Just(pin.clone()), prop_oneof![Just(pin), version()]))
}

/// Two bounds and a release that often sits exactly on one of them.
fn bounds_and_candidate() -> impl Strategy<Value = (Version, Version, Version)> {
    (release(), release()).prop_flat_map(|(low, high)| {
        let candidate = prop_oneof![Just(low.clone()), Just(high.clone()), release()];
        (Just(low), Just(high), candidate)
    })
}

/// Constraint strings in every form `parse` documents.
fn constraint() -> impl Strategy<Value = String> {
    let op = prop_oneof![
        Just(""),
        Just("="),
        Just("^"),
        Just("~"),
        Just(">"),
        Just(">="),
        Just("<"),
        Just("<=")
    ];
    let partial = prop_oneof![
        version().prop_map(|v| v.to_string()),
        (0u64..6, 0u64..6).prop_map(|(major, minor)| format!("{major}.{minor}")),
        (0u64..6).prop_map(|major| major.to_string()),
    ];
    let single = (op, partial)
        .prop_map(|(op, partial)| format!("{op}{partial}"))
        .boxed();
    prop_oneof![
        single.clone(),
        (single.clone(), single).prop_map(|(a, b)| format!("{a}, {b}")),
        Just("*".to_string()),
        Just(String::new()),
        Just("latest".to_string()),
    ]
}

fn satisfies(constraint: &str, version: &Version) -> bool {
    VersionConstraint::parse(constraint)
        .unwrap()
        .satisfies(&version.to_string())
        .unwrap()
}

proptest! {
    /// ADR-0004: a bare `MAJOR.MINOR.PATCH[-pre]` admits that version and nothing else.
    #[test]
    fn a_bare_version_admits_only_itself((pinned, candidate) in pin_and_candidate()) {
        let bare = pinned.to_string();
        prop_assert_eq!(satisfies(&bare, &candidate), candidate == pinned);
        prop_assert!(satisfies(&bare, &pinned));
    }

    /// A bare pin is exact, so it resolves offline without a versions listing.
    #[test]
    fn a_bare_version_is_its_own_exact_version(pinned in version()) {
        let bare = pinned.to_string();
        let parsed = VersionConstraint::parse(&bare).unwrap();
        prop_assert_eq!(parsed.as_exact(), Some(bare.clone()));
        prop_assert_eq!(parsed, VersionConstraint::parse(&format!("={bare}")).unwrap());
    }

    /// Build metadata has no precedence in semver, so it neither narrows a
    /// pin to one build nor stops the pin being exact.
    #[test]
    fn build_metadata_never_changes_a_pin(pinned in version(), build in "[a-z0-9]{1,8}", other in "[a-z0-9]{1,8}") {
        let with_build = VersionConstraint::parse(&format!("{pinned}+{build}")).unwrap();
        prop_assert_eq!(&with_build, &VersionConstraint::parse(&pinned.to_string()).unwrap());
        prop_assert_eq!(with_build.as_exact(), Some(pinned.to_string()));
        let other_build = format!("{pinned}+{other}");
        prop_assert!(with_build.satisfies(&other_build).unwrap());
    }

    /// Whatever `as_exact` names is admitted by its constraint.
    #[test]
    fn an_exact_version_satisfies_its_constraint(text in constraint()) {
        let Ok(parsed) = VersionConstraint::parse(&text) else { return Ok(()) };
        if let Some(exact) = parsed.as_exact() {
            prop_assert!(parsed.satisfies(&exact).unwrap(), "{text} -> {exact}");
        }
    }

    /// The lock file stores `to_string()` and reads it back through `parse`
    /// (and serde); either way the constraint must come back unchanged.
    #[test]
    fn the_canonical_string_round_trips(text in constraint()) {
        let Ok(parsed) = VersionConstraint::parse(&text) else { return Ok(()) };
        let canonical = parsed.to_string();
        prop_assert_eq!(&VersionConstraint::parse(&canonical).unwrap(), &parsed, "{} -> {}", text, canonical);
        let json = serde_json::to_string(&parsed).unwrap();
        prop_assert_eq!(&serde_json::from_str::<VersionConstraint>(&json).unwrap(), &parsed);
    }

    /// Surrounding whitespace never changes meaning.
    #[test]
    fn surrounding_whitespace_is_ignored(text in constraint(), left in "[ \t]{0,3}", right in "[ \t]{0,3}") {
        let padded = format!("{left}{text}{right}");
        prop_assert_eq!(
            VersionConstraint::parse(&padded).ok(),
            VersionConstraint::parse(&text).ok()
        );
    }

    /// BUG-2: caret keeps the left-most non-zero component fixed.
    #[test]
    fn caret_admits_versions_up_to_the_next_breaking_one(base in release(), candidate in release()) {
        let breaking = if base.major > 0 {
            Version::new(base.major + 1, 0, 0)
        } else if base.minor > 0 {
            Version::new(0, base.minor + 1, 0)
        } else {
            Version::new(0, 0, base.patch + 1)
        };
        prop_assert_eq!(
            satisfies(&format!("^{base}"), &candidate),
            candidate >= base && candidate < breaking
        );
    }

    /// `~M.m.p` admits patch releases of `M.m` from `p` up.
    #[test]
    fn tilde_admits_patch_releases_only(base in release(), candidate in release()) {
        prop_assert_eq!(
            satisfies(&format!("~{base}"), &candidate),
            candidate >= base && candidate < Version::new(base.major, base.minor + 1, 0)
        );
    }

    /// BUG-3/4: strict bounds exclude the bound itself, and a comma range is
    /// the intersection of its sides.
    #[test]
    fn comparison_bounds_follow_version_order((low, high, candidate) in bounds_and_candidate()) {
        prop_assert_eq!(satisfies(&format!(">{low}"), &candidate), candidate > low);
        prop_assert_eq!(satisfies(&format!(">={low}"), &candidate), candidate >= low);
        prop_assert_eq!(satisfies(&format!("<{high}"), &candidate), candidate < high);
        prop_assert_eq!(satisfies(&format!("<={high}"), &candidate), candidate <= high);
        prop_assert_eq!(
            satisfies(&format!(">={low},<{high}"), &candidate),
            candidate >= low && candidate < high
        );
    }

    /// A pre-release is never picked up by a range written for releases.
    #[test]
    fn release_ranges_skip_prereleases(base in release(), (mut candidate, pre) in (release(), prerelease())) {
        candidate.pre = semver::Prerelease::new(&pre).unwrap();
        for op in ["^", "~", ">", ">=", "<", "<="] {
            prop_assert!(!satisfies(&format!("{op}{base}"), &candidate), "{op}{base} admitted {candidate}");
        }
        prop_assert!(!satisfies("*", &candidate));
    }

    /// Newest first by semver, every input kept, unparseable entries last.
    #[test]
    fn sorting_is_newest_first(
        versions in prop::collection::vec(
            prop_oneof![
                4 => version().prop_map(|v| v.to_string()),
                1 => "[a-z]{1,6}",
            ],
            0..12,
        )
    ) {
        let mut sorted = versions.clone();
        sort_versions_desc(&mut sorted);

        let mut expected = versions.clone();
        let mut actual = sorted.clone();
        expected.sort();
        actual.sort();
        prop_assert_eq!(actual, expected);

        let parsed: Vec<_> = sorted.iter().map(|v| Version::parse(v).ok()).collect();
        for pair in parsed.windows(2) {
            prop_assert!(pair[0] >= pair[1], "{:?}", sorted);
        }

        let newest = versions.iter().filter_map(|v| Version::parse(v).ok()).max();
        match newest {
            Some(newest) => prop_assert_eq!(newest_version(&versions), Some(newest.to_string())),
            None => prop_assert_eq!(newest_version(&versions).is_some(), !versions.is_empty()),
        }
    }

    /// `compare_versions` is semver order and `is_newer` is its strict part.
    #[test]
    fn comparison_is_semver_order(a in version(), b in version()) {
        let (a_text, b_text) = (a.to_string(), b.to_string());
        prop_assert_eq!(compare_versions(&a_text, &b_text).unwrap(), a.cmp(&b));
        prop_assert_eq!(is_newer(&a_text, &b_text).unwrap(), a.cmp(&b) == Ordering::Greater);
        prop_assert!(!(is_newer(&a_text, &b_text).unwrap() && is_newer(&b_text, &a_text).unwrap()));
    }
}
