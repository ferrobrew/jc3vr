use pretty_assertions::assert_eq;

use super::*;

const EYE: AspectSize = AspectSize {
    width: 2000,
    height: 2000,
};

#[test]
fn a_single_eye_fills_the_window_and_records_its_cropped_rectangle() {
    let window = (1920, 1080);
    let layout = mirror_layout(MirrorView::Left, EYE, window, window);
    let recorded = recorded_layout(MirrorView::Left, &layout, window, window);
    assert_eq!(recorded.view, MirrorView::Left);
    assert_eq!(recorded.window, window);
    assert_eq!(
        recorded.draws,
        [
            Some(MirrorDraw {
                eye: 0,
                rect: [0.0, -420.0, 1920.0, 1920.0],
            }),
            None,
        ]
    );
}

#[test]
fn a_stretched_buffer_records_window_pixels_and_the_pair_in_screen_order() {
    let (buffer, window) = ((2000, 2000), (1000, 500));
    let layout = mirror_layout(MirrorView::StereoPair, EYE, buffer, window);
    let recorded = recorded_layout(MirrorView::StereoPair, &layout, buffer, window);
    assert_eq!(
        recorded.draws,
        [
            Some(MirrorDraw {
                eye: 1,
                rect: [0.0, 0.0, 500.0, 500.0],
            }),
            Some(MirrorDraw {
                eye: 0,
                rect: [500.0, 0.0, 500.0, 500.0],
            }),
        ]
    );
}
