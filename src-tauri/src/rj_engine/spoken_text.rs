pub fn normalize_for_speech(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let mut intermediate = String::with_capacity(text.len());
    let mut skipped_tag_closer = None;

    for (index, character) in characters.iter().copied().enumerate() {
        if let Some(closer) = skipped_tag_closer {
            if character == closer {
                skipped_tag_closer = None;
                intermediate.push(' ');
            }
            continue;
        }
        if character == '[' {
            skipped_tag_closer = Some(']');
            continue;
        }
        if character == '<' {
            skipped_tag_closer = Some('>');
            continue;
        }

        let previous = index
            .checked_sub(1)
            .and_then(|previous| characters.get(previous));
        let next = characters.get(index + 1);
        match character {
            '"' | '“' | '”' | '‘' => {}
            '\'' | '’'
                if previous.is_some_and(|value| value.is_alphanumeric())
                    && next.is_some_and(|value| value.is_alphanumeric()) =>
            {
                intermediate.push('\'');
            }
            '\'' | '’' => {}
            '—' | '–' | '(' | ')' => intermediate.push_str(", "),
            '&' => intermediate.push_str(" and "),
            '%' => intermediate.push_str(" percent "),
            '#' | '_' | '*' | '{' | '}' => intermediate.push(' '),
            value
                if value.is_alphanumeric()
                    || value.is_whitespace()
                    || matches!(value, '.' | ',' | '!' | '?' | ';' | ':' | '-' | '/' | '+') =>
            {
                intermediate.push(value);
            }
            _ => intermediate.push(' '),
        }
    }

    collapse_spoken_spacing(&intermediate)
}

pub fn normalize_generated_dialogue(text: &str, profile_name: &str) -> String {
    let normalized = normalize_for_speech(text);
    let Some((label, dialogue)) = normalized.split_once(':') else {
        return normalized;
    };
    if label.trim().eq_ignore_ascii_case(profile_name.trim()) {
        dialogue.trim_start().to_owned()
    } else {
        normalized
    }
}

fn collapse_spoken_spacing(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.trim().chars() {
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space
            && !normalized.is_empty()
            && !matches!(character, '.' | ',' | '!' | '?' | ';' | ':')
        {
            normalized.push(' ');
        }
        if matches!(character, '!' | '?') && normalized.ends_with(character) {
            pending_space = false;
            continue;
        }
        normalized.push(character);
        pending_space = false;
    }
    normalized.trim_matches([',', ' ']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::{normalize_for_speech, normalize_generated_dialogue};

    #[test]
    fn removes_page_formatting_but_preserves_words_and_contractions() {
        assert_eq!(
            normalize_for_speech("Young the Giant’s ‘Mind Over Matter’ — let’s go! 🎧 #The_Swell"),
            "Young the Giant's Mind Over Matter, let's go! The Swell"
        );
    }

    #[test]
    fn removes_speaker_labels_and_non_spoken_tags() {
        assert_eq!(
            normalize_generated_dialogue(
                "Sanymar: [excited] Keep it moving <break> with the next one.",
                "Sanymar"
            ),
            "Keep it moving with the next one."
        );
    }
}
