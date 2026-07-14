use rofd_dom::{Annotation, AnnotationId, AnnotationModel};

use crate::steps::step_trait::Step;

#[derive(Debug)]
pub struct InsertAnnotationStep {
    pub annotation: Annotation,
}
impl Step for InsertAnnotationStep {
    fn apply(&self, anns: &mut AnnotationModel) {
        anns.insert(self.annotation.clone());
    }
    fn revert(&self, anns: &mut AnnotationModel) {
        anns.remove(&self.annotation.id);
    }
}

#[derive(Debug)]
pub struct DeleteAnnotationStep {
    pub annotation: Annotation,
}
impl Step for DeleteAnnotationStep {
    fn apply(&self, anns: &mut AnnotationModel) {
        anns.remove(&self.annotation.id);
    }
    fn revert(&self, anns: &mut AnnotationModel) {
        anns.insert(self.annotation.clone());
    }
}

#[derive(Debug)]
pub struct ReplaceAnnotationStep {
    pub id: AnnotationId,
    pub before: Annotation,
    pub after: Annotation,
}
impl Step for ReplaceAnnotationStep {
    fn apply(&self, anns: &mut AnnotationModel) {
        if let Some(a) = anns.find_mut(&self.id) {
            *a = self.after.clone();
        }
    }
    fn revert(&self, anns: &mut AnnotationModel) {
        if let Some(a) = anns.find_mut(&self.id) {
            *a = self.before.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_dom::{AnnotationKind, AnnotationPayload, Color, NoteIcon, PageId, Rect};

    fn note_ann(id: &str, content: &str) -> Annotation {
        Annotation {
            id: AnnotationId::new(id),
            kind: AnnotationKind::Note,
            page: PageId::new("P0"),
            creator: "t".into(),
            created: 0,
            modified: 0,
            reply_to: None,
            payload: AnnotationPayload::Note {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: Color::Rgb(0, 0, 0),
                content: content.into(),
                icon: NoteIcon::Note,
            },
        }
    }

    #[test]
    fn insert_then_revert_yields_empty() {
        let mut m = AnnotationModel::default();
        let ann = note_ann("11", "a");
        let step = InsertAnnotationStep {
            annotation: ann.clone(),
        };
        step.apply(&mut m);
        assert!(m.find(&ann.id).is_some());
        step.revert(&mut m);
        assert!(m.find(&ann.id).is_none());
    }

    #[test]
    fn delete_then_revert_restores() {
        let mut m = AnnotationModel::default();
        let ann = note_ann("12", "a");
        m.insert(ann.clone());
        let step = DeleteAnnotationStep {
            annotation: ann.clone(),
        };
        step.apply(&mut m);
        assert!(m.find(&ann.id).is_none());
        step.revert(&mut m);
        assert!(m.find(&ann.id).is_some());
    }

    #[test]
    fn replace_then_revert_restores_before() {
        let mut m = AnnotationModel::default();
        let before = note_ann("13", "before");
        let mut after = before.clone();
        if let AnnotationPayload::Note { content, .. } = &mut after.payload {
            *content = "after".into();
        }
        m.insert(before.clone());
        let step = ReplaceAnnotationStep {
            id: before.id.clone(),
            before: before.clone(),
            after: after.clone(),
        };
        step.apply(&mut m);
        let got = m.find(&before.id).unwrap();
        assert!(
            matches!(&got.payload, AnnotationPayload::Note { content, .. } if content == "after")
        );
        step.revert(&mut m);
        let got = m.find(&before.id).unwrap();
        assert!(
            matches!(&got.payload, AnnotationPayload::Note { content, .. } if content == "before")
        );
    }
}
