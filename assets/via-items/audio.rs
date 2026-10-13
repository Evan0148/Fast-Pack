pub(super) fn get(name: &str) -> Option<&'static [u8]> {
    match name {
        "item/mace/smash_air1.ogg" => Some(include_bytes!("audio/item/mace/smash_air1.ogg")),
        "item/mace/smash_air2.ogg" => Some(include_bytes!("audio/item/mace/smash_air2.ogg")),
        "item/mace/smash_air3.ogg" => Some(include_bytes!("audio/item/mace/smash_air3.ogg")),
        "item/mace/smash_ground1.ogg" => Some(include_bytes!("audio/item/mace/smash_ground1.ogg")),
        "item/mace/smash_ground2.ogg" => Some(include_bytes!("audio/item/mace/smash_ground2.ogg")),
        "item/mace/smash_ground3.ogg" => Some(include_bytes!("audio/item/mace/smash_ground3.ogg")),
        "item/mace/smash_ground4.ogg" => Some(include_bytes!("audio/item/mace/smash_ground4.ogg")),
        "item/mace/smash_ground_heavy.ogg" => Some(include_bytes!("audio/item/mace/smash_ground_heavy.ogg")),
        "random/bow.ogg" => Some(include_bytes!("audio/random/bow.ogg")),
        "entity/wind_charge/wind_burst1.ogg" => Some(include_bytes!("audio/entity/wind_charge/wind_burst1.ogg")),
        "entity/wind_charge/wind_burst2.ogg" => Some(include_bytes!("audio/entity/wind_charge/wind_burst2.ogg")),
        "entity/wind_charge/wind_burst3.ogg" => Some(include_bytes!("audio/entity/wind_charge/wind_burst3.ogg")),
        "mob/breeze/wind_burst1.ogg" => Some(include_bytes!("audio/mob/breeze/wind_burst1.ogg")),
        "mob/breeze/wind_burst2.ogg" => Some(include_bytes!("audio/mob/breeze/wind_burst2.ogg")),
        "mob/breeze/wind_burst3.ogg" => Some(include_bytes!("audio/mob/breeze/wind_burst3.ogg")),
        _ => None,
    }
}
