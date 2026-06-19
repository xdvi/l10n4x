#[cfg(test)]
mod tests {
    use crate::binary_writer::write_binary_format;
    use crate::icu_parser::{MessageNode, MessageParser, PluralCaseKey};
    use l10n4x_core::binary_format::BinaryFormatReader;
    use l10n4x_core::formatter::format_message;
    use std::collections::HashMap;

    #[test]
    fn test_parser_simple() {
        let parser = MessageParser::new("Hello {name}!");
        let nodes = parser.parse().unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0], MessageNode::Text("Hello ".to_string()));
        assert_eq!(nodes[1], MessageNode::Variable("name".to_string()));
        assert_eq!(nodes[2], MessageNode::Text("!".to_string()));
    }

    #[test]
    fn test_parser_plural_mf1() {
        let parser = MessageParser::new(
            "{count, plural, =0 {no messages} =1 {one message} other {# messages}}",
        );
        let nodes = parser.parse().unwrap();
        assert_eq!(nodes.len(), 1);
        if let MessageNode::Plural { var, cases } = &nodes[0] {
            assert_eq!(var, "count");
            assert_eq!(cases.len(), 3);
            assert_eq!(cases[0].0, PluralCaseKey::Exact(0.0));
            assert_eq!(
                cases[0].1,
                vec![MessageNode::Text("no messages".to_string())]
            );

            assert_eq!(cases[1].0, PluralCaseKey::Exact(1.0));
            assert_eq!(
                cases[1].1,
                vec![MessageNode::Text("one message".to_string())]
            );

            assert_eq!(cases[2].0, PluralCaseKey::Other);
            // '#' in MF1 gets translated to Variable(count)
            assert_eq!(
                cases[2].1,
                vec![
                    MessageNode::Variable("count".to_string()),
                    MessageNode::Text(" messages".to_string())
                ]
            );
        } else {
            panic!("Expected Plural node");
        }
    }

    #[test]
    fn test_parser_plural_mf2() {
        let parser = MessageParser::new(
            r#"
            match $count
            when 0 {no messages}
            when one {one message}
            * {{ $count } messages}
        "#,
        );
        let nodes = parser.parse().unwrap();
        assert_eq!(nodes.len(), 1);
        if let MessageNode::Plural { var, cases } = &nodes[0] {
            assert_eq!(var, "count");
            assert_eq!(cases.len(), 3);

            assert_eq!(cases[0].0, PluralCaseKey::Exact(0.0));
            assert_eq!(
                cases[0].1,
                vec![MessageNode::Text("no messages".to_string())]
            );

            assert_eq!(cases[1].0, PluralCaseKey::One);
            assert_eq!(
                cases[1].1,
                vec![MessageNode::Text("one message".to_string())]
            );

            assert_eq!(cases[2].0, PluralCaseKey::Other);
            assert_eq!(
                cases[2].1,
                vec![
                    MessageNode::Variable("count".to_string()),
                    MessageNode::Text(" messages".to_string())
                ]
            );
        } else {
            panic!("Expected Plural node");
        }
    }

    #[test]
    fn test_compiler_core_integration_zero_alloc() {
        // Compile a complex message with plural via binary_writer
        // and validate that BinaryFormatReader + formatter processes it
        let mut translations = HashMap::new();

        let parser = MessageParser::new(
            "{count, plural, =0 {no messages} =1 {one message} other {# messages}}",
        );
        let nodes = parser.parse().unwrap();
        translations.insert("msg.key".to_string(), nodes);

        let binary_bytes = write_binary_format(&translations);

        // Reader lookup (zero-copy)
        let reader = BinaryFormatReader::new(&binary_bytes).unwrap();
        let bytecode = reader.lookup("msg.key").unwrap();

        // Format without allocation using a preallocated writer buffer (represented by String in tests)
        let mut output = String::new();
        let params = [("count", "5")];
        format_message(bytecode, "en", &params, &mut output).unwrap();
        assert_eq!(output, "5 messages");
    }

    #[test]
    fn test_compression_ratio_estimate() {
        let mut translations = HashMap::new();
        // Generate 100 typical translations
        for i in 0..100 {
            let key = format!("module.submodule.action.error_code_{}", i);
            let template = format!("An error occurred during operation {} in module. Please try again or contact support with reference ID: {{ref_id}}.", i);
            let parser = MessageParser::new(&template);
            let nodes = parser.parse().unwrap();
            translations.insert(key, nodes);
        }

        let uncompressed = write_binary_format(&translations);
        let compressed = miniz_oxide::deflate::compress_to_vec(&uncompressed, 6);

        let uncompressed_len = uncompressed.len();
        let compressed_len = compressed.len();
        let reduction = 100.0 - (compressed_len as f64 / uncompressed_len as f64 * 100.0);

        println!(
            "COMPRESSION_TEST_RESULT: Uncompressed: {}, Compressed: {}, Reduction: {:.2}%",
            uncompressed_len, compressed_len, reduction
        );
        assert!(reduction > 50.0);
    }
}
