use zode_app_model::LayoutClass;

#[test]
fn breakpoints_match_design() {
    assert_eq!(LayoutClass::for_width(959.0), LayoutClass::Compact);
    assert_eq!(LayoutClass::for_width(960.0), LayoutClass::Wide);
    assert_eq!(LayoutClass::for_width(719.0), LayoutClass::Phone);
}
