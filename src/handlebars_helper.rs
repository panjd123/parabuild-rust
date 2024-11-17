use handlebars::*;

/// default helper
///
/// Usage: `{{default name 'default_value'}}`
pub fn default_value_helper(
    h: &Helper,
    _: &Handlebars,
    _: &Context,
    _: &mut RenderContext,
    out: &mut dyn Output,
) -> HelperResult {
    let param = h.param(0).unwrap();
    let default = h.param(1).unwrap();

    if param.value().is_null() {
        out.write(default.value().render().as_ref())?;
    } else {
        out.write(param.value().render().as_ref())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_default_helper() {
        let mut hbs = Handlebars::new();

        let data = json!({
            "name": "Turing",
            "age": 42
        });

        hbs.register_helper("default", Box::new(default_value_helper));

        let template = "{{default name 'Alan'}}";
        let rendered = hbs.render_template(template, &data).unwrap();
        assert_eq!(rendered, "Turing");

        let template = "{{default age 99}}";
        let rendered = hbs.render_template(template, &data).unwrap();
        assert_eq!(rendered, "42");

        let template = "{{default missing 'default'}}";
        let rendered = hbs.render_template(template, &data).unwrap();
        assert_eq!(rendered, "default");

        let template = r#"{{default missing "default"}}"#;
        let rendered = hbs.render_template(template, &data).unwrap();
        assert_eq!(rendered, "default");

        // let template = "{{default missing default}}";
        // let rendered = hbs.render_template(template, &data).unwrap();
        // assert_eq!(rendered, "");
    }
}
