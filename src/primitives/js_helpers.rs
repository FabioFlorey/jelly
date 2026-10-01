pub(crate) const TEXTBOX_SELECTOR: &str = "input:not([type]),input[type=text],input[type=search],input[type=email],input[type=password],input[type=url],input[type=number],textarea,[role=textbox],[contenteditable=true]";

pub(crate) const NORMALIZE_TEXT_FN: &str =
    r#"(value) => (value || '').replace(/\s+/g, ' ').trim()"#;

pub(crate) const LAYOUT_VISIBLE_FN: &str = r#"(element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rect.width > 0 &&
        rect.height > 0 &&
        style.display !== 'none' &&
        style.visibility !== 'hidden';
}"#;

pub(crate) const RENDERED_VISIBLE_FN: &str = r#"(element) => {
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rect.width > 0 &&
        rect.height > 0 &&
        style.display !== 'none' &&
        style.visibility !== 'hidden' &&
        style.opacity !== '0';
}"#;

pub(crate) const CENTER_POINT_FN: &str = r#"(element) => {
    const rect = element.getBoundingClientRect();
    return {
        x: rect.left + rect.width / 2,
        y: rect.top + rect.height / 2
    };
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_preserve_expected_visibility_contracts() {
        assert!(!LAYOUT_VISIBLE_FN.contains("opacity"));
        assert!(RENDERED_VISIBLE_FN.contains("opacity"));
        assert!(CENTER_POINT_FN.contains("rect.left + rect.width / 2"));
        assert!(NORMALIZE_TEXT_FN.contains(r"/\s+/g"));
        assert!(TEXTBOX_SELECTOR.contains("[contenteditable=true]"));
    }
}
