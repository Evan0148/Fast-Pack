//! Portable effect rules for clients without NBT list-count support.
use super::*;

pub(super) fn variants(props: &Properties, target: u32, item: &str) -> Vec<(String, Properties)> {
    if !(4..=15).contains(&target)
        || !matches!(
            item,
            "minecraft:potion"
                | "minecraft:splash_potion"
                | "minecraft:lingering_potion"
                | "minecraft:tipped_arrow"
        )
    {
        return vec![];
    }
    // This client's NBT matcher does not implement a list's "count" pseudo-key.
    // Use the first custom effect, while explicit registered potion types take
    // precedence. An omitted Potion tag is valid for a custom-effect potion.
    let Some(potion) = props.get("nbt.Potion") else {
        return vec![];
    };
    let Some(name) = potion.strip_prefix("minecraft:") else {
        return vec![];
    };
    let long = name.starts_with("long_");
    let strong = name.starts_with("strong_");
    let base = name
        .strip_prefix("long_")
        .or_else(|| name.strip_prefix("strong_"))
        .unwrap_or(name);
    // Vanilla 1.20.1 potion registrations and legacy effect IDs. Extended
    // duration variants require their registered duration, never a color guess.
    let (effect, long_duration, supports_strong) = match base {
        "night_vision" => (16, 9600, false),
        "invisibility" => (14, 9600, false),
        "leaping" => (8, 9600, true),
        "fire_resistance" => (12, 9600, false),
        "swiftness" => (1, 9600, true),
        "slowness" => (2, 4800, true),
        "turtle_master" => (2, 800, true),
        "water_breathing" => (13, 9600, false),
        "healing" => (6, 0, true),
        "harming" => (7, 0, true),
        "poison" => (19, 1800, true),
        "regeneration" => (10, 1800, true),
        "strength" => (5, 9600, true),
        "weakness" => (18, 4800, false),
        "luck" => (26, 0, false),
        "slow_falling" => (28, 4800, false),
        _ => return vec![],
    };
    if strong && !supports_strong || long && long_duration == 0 {
        return vec![];
    }
    let amplifier = if base == "turtle_master" {
        if strong { 5 } else { 3 }
    } else if strong {
        if base == "slowness" { 3 } else { 1 }
    } else {
        0
    };
    let mut variant = props.clone();
    variant.remove("nbt.Potion");
    variant.insert("nbt.CustomPotionEffects.0.Id".into(), effect.to_string());
    variant.insert(
        "nbt.CustomPotionEffects.0.Amplifier".into(),
        amplifier.to_string(),
    );
    if long {
        variant.insert(
            "nbt.CustomPotionEffects.0.Duration".into(),
            long_duration.to_string(),
        );
    }
    if base == "turtle_master" {
        variant.insert("nbt.CustomPotionEffects.1.Id".into(), "11".into());
        variant.insert(
            "nbt.CustomPotionEffects.1.Amplifier".into(),
            if strong { "3" } else { "2" }.into(),
        );
    }
    // Every exact type predicate remains ahead of its generic fallback, but a
    // carrier's custom instant effect must win over the carrier's water icon.
    let priority = props
        .get("weight")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    variant.insert(
        "weight".into(),
        (priority
            + 100000
            + if long { 10000 } else { 0 }
            + if base == "turtle_master" { 5000 } else { 0 })
        .to_string(),
    );
    vec![(format!("effect-{name}"), variant)]
}

pub(super) fn prioritize(props: &mut Properties, target: u32, item: &str) {
    if !(4..=15).contains(&target)
        || !matches!(
            item,
            "minecraft:potion"
                | "minecraft:splash_potion"
                | "minecraft:lingering_potion"
                | "minecraft:tipped_arrow"
        )
    {
        return;
    }
    if props.get("nbt.Potion").is_some_and(|value| {
        value.starts_with("minecraft:")
            && ![
                "minecraft:water",
                "minecraft:mundane",
                "minecraft:thick",
                "minecraft:awkward",
                "minecraft:empty",
            ]
            .contains(&value.as_str())
    }) {
        if let Some(weight) = props
            .get("weight")
            .and_then(|value| value.parse::<u32>().ok())
        {
            props.insert("weight".into(), weight.saturating_add(200000).to_string());
        }
    }
}
