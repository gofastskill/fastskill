//! Typed access to process-wide CLI arguments during command dispatch.

use cli_framework::app::context::AppContext;
use cli_framework::spec::value::ArgValue;
use std::path::PathBuf;

pub(crate) fn global(ctx: &dyn AppContext) -> bool {
    ctx.opt_global_args()
        .and_then(|arguments| arguments.get("global"))
        .and_then(|value| match value {
            ArgValue::Bool(enabled) => Some(*enabled),
            _ => None,
        })
        .unwrap_or(false)
}

pub(crate) fn skills_directory(ctx: &dyn AppContext) -> Option<PathBuf> {
    ctx.opt_global_args()
        .and_then(|arguments| arguments.get("skills-dir"))
        .and_then(|value| match value {
            ArgValue::Str(path) => Some(PathBuf::from(path)),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct ArgsContext(Option<HashMap<String, ArgValue>>);

    impl AppContext for ArgsContext {
        fn opt_global_args(&self) -> Option<&HashMap<String, ArgValue>> {
            self.0.as_ref()
        }
    }

    #[test]
    fn reads_typed_global_arguments_and_defaults_missing_values() {
        let context = ArgsContext(Some(HashMap::from([
            ("global".to_string(), ArgValue::Bool(true)),
            (
                "skills-dir".to_string(),
                ArgValue::Str("custom-skills".to_string()),
            ),
        ])));
        assert!(global(&context));
        assert_eq!(
            skills_directory(&context),
            Some(PathBuf::from("custom-skills"))
        );

        let missing = ArgsContext(None);
        assert!(!global(&missing));
        assert_eq!(skills_directory(&missing), None);
    }

    #[test]
    fn ignores_values_with_the_wrong_types() {
        let context = ArgsContext(Some(HashMap::from([
            ("global".to_string(), ArgValue::Str("yes".to_string())),
            ("skills-dir".to_string(), ArgValue::Bool(true)),
        ])));
        assert!(!global(&context));
        assert_eq!(skills_directory(&context), None);
    }
}
