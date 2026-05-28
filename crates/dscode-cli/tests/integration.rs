use dscode::api;

#[test]
fn test_resolve_model_name() {
    assert_eq!(api::resolve_model_name("v4-pro"), "deepseek-v4-pro");
    assert_eq!(api::resolve_model_name("v4pro"), "deepseek-v4-pro");
    assert_eq!(api::resolve_model_name("flash"), "deepseek-v4-flash");
    assert_eq!(api::resolve_model_name("v4-flash"), "deepseek-v4-flash");
    assert_eq!(api::resolve_model_name("r1"), "deepseek-r1");
    assert_eq!(api::resolve_model_name("v3"), "deepseek-v3");
    assert_eq!(api::resolve_model_name("chat"), "deepseek-chat");
    assert_eq!(api::resolve_model_name("deepseek-v4-pro"), "deepseek-v4-pro");
    assert_eq!(api::resolve_model_name(""), "");
    assert_eq!(api::resolve_model_name("custom"), "custom");
    assert_eq!(api::resolve_model_name("v3.2"), "deepseek-v3.2");
    assert_eq!(api::resolve_model_name("v4flash"), "deepseek-v4-flash");
    assert_eq!(api::resolve_model_name("reasoner"), "deepseek-reasoner");
    assert_eq!(api::resolve_model_name("coder"), "deepseek-coder");
}

#[test]
fn test_resolve_model_case_sensitive() {
    assert_eq!(api::resolve_model_name("V4-PRO"), "V4-PRO");
    assert_eq!(api::resolve_model_name("FLASH"), "FLASH");
    assert_eq!(api::resolve_model_name("R1"), "R1");
}
