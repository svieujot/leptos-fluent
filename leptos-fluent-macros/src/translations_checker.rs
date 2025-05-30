use crate::fluent_entries::{build_fluent_entries, FluentEntries};
use crate::tr_macros::{gather_tr_macro_defs_from_rs_files, TranslationMacro};
use crate::{FluentFilePaths, FluentResources};
use std::path::Path;

#[cfg_attr(feature = "tracing", tracing::instrument(level = "trace", skip_all))]
pub(crate) fn run(
    globstr: &str,
    workspace_path: impl AsRef<Path>,
    fluent_resources: &FluentResources,
    fluent_file_paths: &FluentFilePaths,
    core_locales_path: &Option<String>,
    core_locales_content: &Option<String>,
) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();

    let ws_path = workspace_path.as_ref();
    let (tr_macros, tr_macros_errors) = gather_tr_macro_defs_from_rs_files(
        ws_path.join(globstr),
        #[cfg(not(test))]
        ws_path,
    );

    #[cfg(feature = "tracing")]
    if !tr_macros_errors.is_empty() {
        tracing::warn!(
            "Errors while gathering tr macros: {:#?}",
            tr_macros_errors
        );
    } else {
        tracing::trace!("Gathered tr macros: {:#?}", tr_macros);
    }

    errors.extend(tr_macros_errors);

    let (fluent_entries, fluent_syntax_errors) = build_fluent_entries(
        fluent_resources,
        fluent_file_paths,
        &workspace_path,
        core_locales_path,
        core_locales_content,
    );

    #[cfg(feature = "tracing")]
    if !&fluent_syntax_errors.is_empty() {
        tracing::warn!(
            "Errors while building fluent entries: {:#?}",
            &fluent_syntax_errors
        );
    } else {
        tracing::trace!("Built fluent entries: {:#?}", fluent_entries);
    }

    errors.extend(fluent_syntax_errors);

    let mut check_messages =
        check_tr_macros_against_fluent_entries(&tr_macros, &fluent_entries);
    check_messages.extend(check_fluent_entries_against_tr_macros(
        &tr_macros,
        &fluent_entries,
    ));

    // TODO: Currently, the fluent-syntax parser does not offer a CST
    //       parser so we don't know the spans of the entries.
    //       See https://github.com/projectfluent/fluent-rs/issues/270

    #[cfg(feature = "tracing")]
    if !check_messages.is_empty() {
        tracing::warn!(
            "Errors while checking translations: {:#?}",
            &check_messages
        );
    }

    (check_messages, errors)
}

fn macro_location(tr_macro: &TranslationMacro) -> String {
    let file_path = {
        #[cfg(not(test))]
        {
            &tr_macro.file_path
        }

        #[cfg(test)]
        {
            _ = tr_macro;
            "[test content]"
        }
    };

    #[cfg(not(feature = "nightly"))]
    {
        file_path.to_string()
    }

    #[cfg(feature = "nightly")]
    {
        if tr_macro.start.line == 0 && tr_macro.start.column == 0 {
            file_path.to_string()
        } else {
            format!(
                "{}:{}:{}",
                &file_path, tr_macro.start.line, tr_macro.start.column,
            )
        }
    }
}

fn check_tr_macros_against_fluent_entries(
    tr_macros: &Vec<TranslationMacro>,
    fluent_entries: &FluentEntries,
) -> Vec<String> {
    let mut error_messages: Vec<String> = Vec::new();

    for tr_macro in tr_macros {
        for (lang, entries) in fluent_entries {
            // tr macro message must be defined for each language
            let mut message_name_found = false;
            for entry in entries {
                if tr_macro.message_name == entry.message_name {
                    message_name_found = true;

                    // Check if all variables in the tr macro are present in the fluent entry
                    for placeable in &tr_macro.placeables {
                        if !entry.placeables.contains(placeable) {
                            let error_message = format!(
                                concat!(
                                    r#"Variable "{}" defined at {} macro"#,
                                    r#" call in {} not found in message"#,
                                    r#" "{}" of locale "{}"."#,
                                ),
                                placeable,
                                format_macro_call(tr_macro),
                                macro_location(tr_macro),
                                tr_macro.message_name,
                                lang,
                            );

                            error_messages.push(error_message);
                        }
                    }

                    break;
                }
            }
            if !message_name_found {
                let error_message = if check_tr_macro_message_name_is_valid(
                    &tr_macro.message_name,
                ) {
                    format!(
                        concat!(
                            r#"Message "{}" defined at {} macro call in {}"#,
                            r#" not found in files for locale "{}"."#,
                        ),
                        tr_macro.message_name,
                        format_macro_call(tr_macro),
                        macro_location(tr_macro),
                        lang,
                    )
                } else {
                    format!(
                        concat!(
                            r#"Invalid message identifier "{}" defined at"#,
                            r#" {} macro call in {} for locale "{}"."#,
                            " Fluent message identifiers must match the",
                            " regular expression '[a-zA-Z][a-zA-Z0-9_-]+'.",
                        ),
                        tr_macro.message_name,
                        format_macro_call(tr_macro),
                        macro_location(tr_macro),
                        lang,
                    )
                };

                error_messages.push(error_message);
            }
        }
    }
    error_messages
}

fn check_fluent_entries_against_tr_macros(
    tr_macros: &Vec<TranslationMacro>,
    fluent_entries: &FluentEntries,
) -> Vec<String> {
    let mut error_messages: Vec<String> = Vec::new();

    for (lang, entries) in fluent_entries {
        for entry in entries {
            // fluent entry message must be defined for each language
            let mut message_name_found = false;
            for tr_macro in tr_macros {
                if tr_macro.message_name == entry.message_name {
                    message_name_found = true;

                    // Check if all variables in the entry are present in the tr macro
                    for placeable in &entry.placeables {
                        if !tr_macro.placeables.contains(placeable) {
                            error_messages.push(
                                format!(
                                    concat!(
                                        r#"Variable "{}" defined in message "{}" of"#,
                                        r#" locale "{}" not found in arguments of"#,
                                        r#" {} macro call at file {}."#,
                                    ),
                                    placeable,
                                    entry.message_name,
                                    lang,
                                    format_macro_call(tr_macro),
                                    macro_location(tr_macro),
                                )
                            );
                        }
                    }

                    break;
                }
            }
            if !message_name_found {
                let error_message = format!(
                    concat!(
                        r#"Message "{}" of locale "{}" not found in any"#,
                        r#" `tr!` or `move_tr!` macro calls."#,
                    ),
                    entry.message_name, lang,
                );
                error_messages.push(error_message);
            }
        }
    }
    error_messages
}

fn format_macro_call(tr_macro: &TranslationMacro) -> String {
    let macro_name = &tr_macro.name;
    let message_name = &tr_macro.message_name;
    if !tr_macro.placeables.is_empty() {
        return format!(r#"`{macro_name}!("{message_name}", {{ ... }})`"#);
    }
    format!(r#"`{macro_name}!("{message_name}")`"#)
}

/// Check if the message name is a valid Fluent message identifier.
///
/// See the Fluent EBNF grammar for message identifiers:
/// https://github.com/projectfluent/fluent/blob/fd8f95478e29dda8121da7e275d375eb8dadbcb0/spec/fluent.ebnf
fn check_tr_macro_message_name_is_valid(message_name: &str) -> bool {
    let mut chars = message_name.chars();
    if !chars.next().unwrap_or('0').is_ascii_alphabetic() {
        return false;
    }
    loop {
        match chars.next() {
            Some(character) => {
                if !character.is_ascii_alphanumeric()
                    && !['_', '-'].contains(&character)
                {
                    return false;
                }
            }
            None => return true,
        }
    }
}
