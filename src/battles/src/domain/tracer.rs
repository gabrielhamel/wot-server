use glam::Vec3;

const DRAW_DISTANCE: f32 = 565.0;
const MAX_SPOTTING_DISTANCE: f32 = 445.0;
const HIDDEN_TRACER_COSINE: f32 = 0.906_307_8; // cos(25°)

#[derive(Clone, Copy)]
pub(crate) enum TracerView {
    Direct {
        direction: Vec3,
    },
    #[allow(
        dead_code,
        reason = "the strategic camera is not wired to Training yet"
    )]
    Strategic {
        contains_origin: bool,
    },
}

pub(crate) fn visible(origin: Vec3, viewer: Vec3, source_known: bool, view: TracerView) -> bool {
    let offset = origin - viewer;
    let distance = offset.length_squared();
    if source_known && distance <= DRAW_DISTANCE.powi(2) {
        return true;
    }
    match view {
        TracerView::Strategic { contains_origin } => contains_origin,
        TracerView::Direct { direction } => {
            !source_known
                && distance <= MAX_SPOTTING_DISTANCE.powi(2)
                && (distance == 0.0
                    || direction
                        .normalize_or_zero()
                        .dot(offset.normalize_or_zero())
                        >= HIDDEN_TRACER_COSINE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sources_use_draw_range_and_hidden_sources_use_view() {
        let direct = TracerView::Direct { direction: Vec3::Z };
        let seen = |point, known, view| visible(point, Vec3::ZERO, known, view);
        assert!(seen(Vec3::X * 565.0, true, direct));
        assert!(!seen(Vec3::Z * 566.0, true, direct));
        assert!(seen(Vec3::Z * 445.0, false, direct));
        assert!(!seen(Vec3::Z * 446.0, false, direct));
        assert!(!seen(Vec3::X * 10.0, false, direct));
        assert!(seen(
            Vec3::X * 550.0,
            false,
            TracerView::Strategic {
                contains_origin: true
            }
        ));
        assert!(!seen(
            Vec3::X * 550.0,
            false,
            TracerView::Strategic {
                contains_origin: false
            }
        ));
    }
}
