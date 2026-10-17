//! Equivalent instant-effect potion appearances for servers sending custom
//! effects on a water/empty base, rather than a registered healing/harming type.
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
    // Do not infer type from names or RGB colors. Only a single, explicit
    // instant effect is equivalent; mixtures retain the authored fallback.
    let Some(potion) = props.get("nbt.Potion") else {
        return vec![];
    };
    let (effect, amplifier) = match potion.as_str() {
        "minecraft:healing" => (6, 0),
        "minecraft:strong_healing" => (6, 1),
        "minecraft:harming" => (7, 0),
        "minecraft:strong_harming" => (7, 1),
        _ => return vec![],
    };
    let mut variant = props.clone();
    variant.insert(
        "nbt.Potion".into(),
        "regex:minecraft:(?:water|mundane|thick|awkward|empty)".into(),
    );
    variant.insert("nbt.CustomPotionEffects.count".into(), "1".into());
    variant.insert("nbt.CustomPotionEffects.0.Id".into(), effect.to_string());
    variant.insert(
        "nbt.CustomPotionEffects.0.Amplifier".into(),
        amplifier.to_string(),
    );
    // Every exact type predicate remains ahead of its generic fallback, but a
    // carrier's custom instant effect must win over the carrier's water icon.
    let priority = props
        .get("weight")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    variant.insert("weight".into(), (priority + 100000).to_string());
    vec![(format!("effect-{effect}-{amplifier}"), variant)]
}
