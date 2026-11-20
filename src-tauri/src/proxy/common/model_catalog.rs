//! Model discovery groups; concrete IDs remain available to the request gate and router.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) const EFFORT_SUFFIXES: &[&str] = &[
    "extra-low",
    "x-high",
    "xhigh",
    "medium",
    "high",
    "low",
    "max",
];

pub(crate) fn effort_base(id: &str) -> Option<&str> {
    EFFORT_SUFFIXES.iter().find_map(|effort| {
        id.strip_suffix(effort)
            .and_then(|prefix| prefix.strip_suffix('-'))
            .filter(|base| !base.is_empty())
    })
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct ModelGroup {
    pub id: String,
    pub members: Vec<String>,
}

/// A list may already contain account models before the runtime official catalog is synced.
/// Group a known effort suffix only when its base is also exposed, never across custom aliases.
pub fn group_models(ids: Vec<String>, aliases: &HashMap<String, String>) -> Vec<ModelGroup> {
    let aliases: HashSet<_> = aliases.keys().map(|id| id.to_ascii_lowercase()).collect();
    let ids: BTreeSet<_> = ids.into_iter().collect();
    let mut canonical_ids = HashMap::new();
    for id in &ids {
        canonical_ids.entry(id.to_ascii_lowercase()).or_insert(id);
    }
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in &ids {
        let key = id.to_ascii_lowercase();
        let base = effort_base(&key).filter(|base| {
            !aliases.contains(&key)
                && !aliases.contains(*base)
                && !base.contains("image")
                && !base.starts_with("imagen")
        });
        let group_id = base
            .and_then(|base| canonical_ids.get(base).copied())
            .unwrap_or(id);
        groups.entry(group_id.clone()).or_default().push(id.clone());
    }
    groups
        .into_iter()
        .map(|(id, members)| ModelGroup { id, members })
        .collect()
}

/// If the base is disabled, keep listing the explicitly allowed legacy variants so
/// discovery never advertises an ID that the request gate would reject.
pub fn discoverable_ids(groups: Vec<ModelGroup>, is_enabled: impl Fn(&str) -> bool) -> Vec<String> {
    let mut ids = Vec::new();
    for group in groups {
        if is_enabled(&group.id) {
            ids.push(group.id);
        } else {
            ids.extend(group.members.into_iter().filter(|id| is_enabled(id)));
        }
    }
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_only_reasoning_variants_and_keeps_tiered_independent() {
        let ids = [
            "gemini-3.8-flash",
            "gemini-3.8-flash-low",
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-extra-low",
            "gemini-3.8-flash-tiered",
            "gemini-4-hydrogen-tiered",
            "gemini-4-hydrogen-tiered-high",
            "gpt-oss-120b",
            "gpt-oss-120b-medium",
            "custom-high",
            "gemini-3.8-flash-lite",
            "gemini-3.8-flash-image-4k",
        ]
        .map(str::to_owned)
        .to_vec();
        let aliases = HashMap::from([("gemini-3.8-flash-high".to_owned(), "other".to_owned())]);
        let groups = group_models(ids, &aliases);
        let members = |id: &str| &groups.iter().find(|g| g.id == id).unwrap().members;
        assert_eq!(
            members("gemini-3.8-flash"),
            &[
                "gemini-3.8-flash",
                "gemini-3.8-flash-extra-low",
                "gemini-3.8-flash-low"
            ]
        );
        assert_eq!(
            members("gemini-4-hydrogen-tiered"),
            &["gemini-4-hydrogen-tiered", "gemini-4-hydrogen-tiered-high"]
        );
        for id in [
            "gemini-3.8-flash-tiered",
            "custom-high",
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-lite",
            "gemini-3.8-flash-image-4k",
        ] {
            assert_eq!(members(id), &[id]);
        }
    }

    #[test]
    fn groups_account_models_on_cold_start_before_catalog_sync() {
        let ids = [
            "gemini-4-hydrogen",
            "gemini-4-hydrogen-low",
            "gemini-4-hydrogen-medium",
            "gemini-4-hydrogen-high",
            "gemini-4-hydrogen-xhigh",
            "gemini-4-hydrogen-tiered",
            "gemini-4-hydrogen-tiered-high",
            "gemini-4-hydrogen-tiered-xhigh",
            "gemini-4-neon",
            "gemini-4-neon-low",
            "gemini-4-neon-medium",
            "gemini-4-neon-high",
            "gemini-4-neon-xhigh",
            "gpt-oss-120b",
            "gpt-oss-120b-medium",
        ];
        let groups = group_models(ids.map(str::to_owned).to_vec(), &HashMap::new());
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.id.as_str(), g.members.len()))
                .collect::<Vec<_>>(),
            [
                ("gemini-4-hydrogen", 5),
                ("gemini-4-hydrogen-tiered", 3),
                ("gemini-4-neon", 5),
                ("gpt-oss-120b", 2),
            ]
        );
    }

    #[test]
    fn discovery_hides_variants_without_widening_a_legacy_allowlist() {
        let ids = [
            "gemini-3.8-flash",
            "gemini-3.8-flash-low",
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-tiered",
        ];
        let make_groups = || group_models(ids.map(str::to_owned).to_vec(), &HashMap::new());
        assert_eq!(
            discoverable_ids(make_groups(), |_| true),
            ["gemini-3.8-flash", "gemini-3.8-flash-tiered"]
        );
        assert_eq!(
            discoverable_ids(make_groups(), |id| id == "gemini-3.8-flash-low"),
            ["gemini-3.8-flash-low"]
        );
        assert!(discoverable_ids(make_groups(), |_| false).is_empty());
    }
}
