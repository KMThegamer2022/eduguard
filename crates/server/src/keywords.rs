//! EduGuard's own keyword libraries (A2). These are illustrative seed terms —
//! schools curate their own sets in the console; nothing proprietary is
//! redistributed here. Categories follow common safeguarding taxonomy.

use protocol::{KeywordRule, Severity};

pub fn seed_library() -> Vec<KeywordRule> {
    let rows: &[(&str, &str, Severity, &str)] = &[
        // --- self harm -----------------------------------------------------
        ("kill myself", "self_harm", Severity::Critical, "Direct suicidal statement"),
        ("end my life", "self_harm", Severity::Critical, "Direct suicidal statement"),
        ("hurt myself", "self_harm", Severity::High, "Self-harm ideation"),
        ("self harm", "self_harm", Severity::High, "Self-harm reference"),
        ("self-harm", "self_harm", Severity::High, "Self-harm reference"),
        ("cutting myself", "self_harm", Severity::High, "Self-harm reference"),
        ("suicide", "self_harm", Severity::High, "Suicide reference"),
        ("suicidal", "self_harm", Severity::High, "Suicidal ideation"),
        ("overdose", "self_harm", Severity::Medium, "Possible overdose reference"),
        ("not worth living", "self_harm", Severity::High, "Hopelessness statement"),
        // --- bullying ------------------------------------------------------
        ("you're ugly", "bullying", Severity::Low, "Direct insult"),
        ("you are ugly", "bullying", Severity::Low, "Direct insult"),
        ("loser", "bullying", Severity::Low, "Insult"),
        ("no one likes you", "bullying", Severity::Medium, "Exclusionary statement"),
        ("go die", "bullying", Severity::High, "Abusive statement"),
        ("i hate you", "bullying", Severity::Medium, "Hostile statement"),
        ("stupid idiot", "bullying", Severity::Low, "Insult"),
        // --- extremism -----------------------------------------------------
        ("isis", "extremism", Severity::High, "Extremist organisation reference"),
        ("terrorist attack", "extremism", Severity::High, "Violent extremist reference"),
        ("white power", "extremism", Severity::High, "Hate symbol slogan"),
        ("racial slur", "extremism", Severity::Medium, "Potential hate speech"),
        ("gas the", "extremism", Severity::Critical, "Genocidal language"),
        ("nazi", "extremism", Severity::Medium, "Hate ideology reference"),
        // --- drugs ---------------------------------------------------------
        ("buy drugs", "drugs", Severity::High, "Drug transaction language"),
        ("weed dealer", "drugs", Severity::Medium, "Drug procurement language"),
        ("vape shop", "drugs", Severity::Low, "Age-restricted product"),
        ("how to make meth", "drugs", Severity::Critical, "Drug manufacture instruction"),
        ("cocaine", "drugs", Severity::Medium, "Controlled substance"),
        ("alcohol for sale", "drugs", Severity::Medium, "Age-restricted sale"),
        // --- violence ------------------------------------------------------
        ("bomb threat", "violence", Severity::Critical, "Threat language"),
        ("shoot up the", "violence", Severity::Critical, "Mass violence language"),
        ("bring a knife", "violence", Severity::High, "Weapons reference"),
        ("how to make a bomb", "violence", Severity::Critical, "Weapons manufacture"),
        ("i will hurt you", "violence", Severity::High, "Direct threat"),
        ("fight me", "violence", Severity::Low, "Aggressive challenge"),
        // --- grooming / csa ------------------------------------------------
        ("send nudes", "grooming", Severity::Critical, "Sextortion/grooming indicator"),
        ("age of consent", "grooming", Severity::Medium, "Sexual content reference"),
        ("don't tell your parents", "grooming", Severity::High, "Secrecy language"),
        ("secrets between us", "grooming", Severity::High, "Secrecy language"),
        // --- mental wellbeing ----------------------------------------------
        ("nobody cares", "wellbeing", Severity::Low, "Distress statement"),
        ("i want to disappear", "wellbeing", Severity::High, "Distress statement"),
        ("i'm so tired of everything", "wellbeing", Severity::Medium, "Distress statement"),
    ];
    rows.iter()
        .map(|(k, c, s, n)| KeywordRule {
            keyword: (*k).to_string(),
            category: (*c).to_string(),
            severity: *s,
            note: (*n).to_string(),
        })
        .collect()
}

pub fn seed_json() -> String {
    serde_json::to_string_pretty(&seed_library()).expect("seed serializes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_sorted_and_unique() {
        let lib = seed_library();
        assert!(lib.len() > 30);
        let mut seen = std::collections::HashSet::new();
        for rule in &lib {
            assert!(seen.insert(rule.keyword.clone()), "dup {}", rule.keyword);
            assert!(!rule.category.is_empty());
        }
    }
}
