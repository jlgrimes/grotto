use rand::Rng;

const ADJECTIVES: &[&str] = &[
    "crimson", "silent", "bright", "swift", "golden",
    "amber", "azure", "bold", "calm", "dark",
    "deep", "eager", "faint", "gentle", "grand",
    "hazy", "icy", "jade", "keen", "lush",
    "misty", "noble", "pale", "quick", "rosy",
    "rusty", "sandy", "sharp", "shy", "slim",
    "soft", "stark", "steep", "still", "stout",
    "sunny", "tame", "tart", "tiny", "vast",
    "vivid", "warm", "weary", "wild", "wiry",
    "young", "dusty", "fresh", "mossy", "stormy",
];

const NOUNS: &[&str] = &[
    "coral", "tide", "reef", "crab", "wave",
    "pearl", "shell", "kelp", "dune", "gull",
    "foam", "dock", "cove", "cape", "mast",
    "hull", "keel", "oar", "buoy", "knot",
    "sail", "helm", "wake", "surf", "sand",
    "cliff", "isle", "bay", "fin", "tern",
    "seal", "pike", "bass", "cod", "wren",
    "lark", "hare", "fawn", "moth", "newt",
    "fern", "moss", "bark", "root", "vine",
    "reed", "pond", "glen", "dale", "ridge",
];

/// Generate a semantic session ID in `adjective-noun-noun` format.
pub fn generate_session_id() -> String {
    let mut rng = rand::rng();
    let adj = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
    let noun1 = NOUNS[rng.random_range(0..NOUNS.len())];
    let mut noun2 = NOUNS[rng.random_range(0..NOUNS.len())];
    // Avoid duplicate nouns
    while noun2 == noun1 {
        noun2 = NOUNS[rng.random_range(0..NOUNS.len())];
    }
    format!("{}-{}-{}", adj, noun1, noun2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_three_part_id() {
        let id = generate_session_id();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 3, "id should be adjective-noun-noun: {}", id);
    }

    #[test]
    fn id_parts_are_valid_words() {
        let id = generate_session_id();
        let parts: Vec<&str> = id.split('-').collect();
        assert!(ADJECTIVES.contains(&parts[0]), "unknown adjective: {}", parts[0]);
        assert!(NOUNS.contains(&parts[1]), "unknown noun: {}", parts[1]);
        assert!(NOUNS.contains(&parts[2]), "unknown noun: {}", parts[2]);
    }

    #[test]
    fn nouns_are_distinct() {
        // Run multiple times to test the dedup logic
        for _ in 0..20 {
            let id = generate_session_id();
            let parts: Vec<&str> = id.split('-').collect();
            assert_ne!(parts[1], parts[2], "nouns should be distinct: {}", id);
        }
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<String> = (0..10).map(|_| generate_session_id()).collect();
        ids.sort();
        ids.dedup();
        // With 50*50*49 = 122,500 possibilities, 10 should all be unique
        assert_eq!(ids.len(), 10);
    }
}
