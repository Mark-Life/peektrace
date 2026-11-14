//! The create, edit and delete dialogs and the toast stack, built into the
//! overlay layer. Pop-ups a dialog opens go above its panel.

use rune::{El, Tx, col, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog as KitDialog, TextArea, TextInput, Toast,
    ToastRegion, glyphs,
};

use super::paint::{amber_300, amber_400, emerald_400, red_400};
use crate::model::memory::{
    MEMORY_TYPES, ProjectOption, conflict_text, create_description, delete_description,
    project_options,
};
use crate::state::Ui;
use crate::state_memory::{CreateTarget, DeleteForm, Dialog, EditForm, ToastKind};
use crate::theme::{Theme, ty};
use crate::views::widgets::select;

const WIDTH: f32 = 512.0;

fn field(label: &str, control: El, t: &Theme) -> El {
    col()
        .w_full()
        .gap(6.0)
        .child(text(label.to_owned()).font(ty::SM_MEDIUM).color(t.fg))
        .child(control)
}

fn footer(cancel: El, submit: El) -> El {
    row()
        .w_full()
        .gap(8.0)
        .pt(8.0)
        .justify_end()
        .child(cancel)
        .child(submit)
}

fn error_line(key: &str, e: Option<&String>) -> Option<El> {
    e.map(|e| {
        text(e.clone())
            .font(ty::SM)
            .color(red_400())
            .wrap()
            .key(key.to_owned())
    })
}

/// A type select's key, current value and whether it is open.
struct TypeField<'a>(&'a str, &'a str, bool);

fn type_select(
    TypeField(key, current, open): TypeField<'_>,
    t: &Theme,
    layer: &mut Vec<El>,
    toggle: impl Fn(&mut Ui) + 'static,
    close: impl Fn(&mut Ui) + 'static,
    choose: impl Fn(&mut Ui, &str) + Clone + 'static,
) -> El {
    // An unknown type (the `42` file) matches no option and shows empty.
    let current = MEMORY_TYPES.iter().copied().find(|k| *k == current);
    select(key, &MEMORY_TYPES)
        .current(current)
        .placeholder("")
        .label("Type")
        .open(open)
        .on_toggle(toggle)
        .on_close(close)
        .on_choose(move |s: &mut Ui, picked: &str, _tx: &mut Tx| choose(s, picked))
        .build(&t.kit, layer)
        .key(key.to_owned())
        .w_full()
}

fn project_picker(s: &Ui, picked: Option<&ProjectOption>, t: &Theme, layer: &mut Vec<El>) -> El {
    let labels = s
        .memory
        .all()
        .map(|a| a.labels.as_slice())
        .unwrap_or_default();
    let headers = s.rail.listing.as_ref().map_or(&[][..], |l| &l.headers);
    let options = project_options(headers, labels);
    let loading = options.is_empty() && s.rail.listing.is_none() && s.list_error.is_none();
    let placeholder = if loading {
        "Loading projects…"
    } else if options.is_empty() {
        "No Claude projects found"
    } else {
        "Choose a project"
    };
    let names: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    let open = s.memory.draft(&CreateTarget::Pick).project_open;
    let choices = options.clone();
    let select = select("mem-create-project", &names)
        .current(picked.map(|p| p.label.as_str()))
        .placeholder(placeholder)
        .label("Project")
        .disabled(options.is_empty())
        .open(open)
        .on_toggle(|s: &mut Ui| {
            let d = s.memory.draft_mut(&CreateTarget::Pick);
            d.project_open = !d.project_open;
        })
        .on_close(|s: &mut Ui| s.memory.draft_mut(&CreateTarget::Pick).project_open = false)
        .on_choose(move |s: &mut Ui, label: &str, _tx: &mut Tx| {
            let d = s.memory.draft_mut(&CreateTarget::Pick);
            d.picked = choices.iter().find(|o| o.label == label).cloned();
            d.project_open = false;
        })
        .build(&t.kit, layer)
        .key("mem-create-project")
        .w_full();
    let mut c = col().w_full().gap(6.0).child(field("Project", select, t));
    if options.is_empty() {
        if s.list_error.is_some() {
            c = c.child(
                text("Could not load the session list.")
                    .font(ty::XS)
                    .color(red_400()),
            );
        } else if s.rail.listing.is_some() {
            c = c.child(
                text("Memory needs a Claude project. Start a Claude session in a project first.")
                    .font(ty::XS)
                    .color(t.muted_fg)
                    .wrap(),
            );
        }
    }
    c
}

fn create(s: &Ui, target: &CreateTarget, t: &Theme, layer: &mut Vec<El>) {
    let d = s.memory.draft(target);
    let (project, label) = match target {
        CreateTarget::Fixed(slug) => (Some(slug.clone()), Some(slug.clone())),
        CreateTarget::Pick => (
            d.picked.as_ref().map(|p| p.slug.clone()),
            d.picked.as_ref().map(|p| p.label.clone()),
        ),
    };
    let mut popups = Vec::new();
    let mut body = col().w_full().gap(12.0).px(16.0).pb(16.0);
    if *target == CreateTarget::Pick {
        body = body.child(project_picker(s, d.picked.as_ref(), t, &mut popups));
    }
    let tg = target.clone();
    let name = TextInput::new(&d.name)
        .placeholder("api-conventions")
        .on_edit(move |s: &mut Ui| &mut s.memory.draft_mut(&tg).name)
        .build(&t.kit)
        .key("mem-create-name")
        .w_full();
    let tg = target.clone();
    let description = TextInput::new(&d.description)
        .placeholder("One-line hook for the index")
        .on_edit(move |s: &mut Ui| &mut s.memory.draft_mut(&tg).description)
        .build(&t.kit)
        .key("mem-create-description")
        .w_full();
    let (t1, t2, t3) = (target.clone(), target.clone(), target.clone());
    let kind = type_select(
        TypeField("mem-create-type", &d.kind, d.kind_open),
        t,
        &mut popups,
        move |s| {
            let d = s.memory.draft_mut(&t1);
            d.kind_open = !d.kind_open;
        },
        move |s| s.memory.draft_mut(&t2).kind_open = false,
        move |s, k| {
            let d = s.memory.draft_mut(&t3);
            k.clone_into(&mut d.kind);
            d.kind_open = false;
        },
    );
    let tg = target.clone();
    let text_body = TextArea::new(&d.body)
        .placeholder("# Title\n\nUse [[other-memory]] to link.")
        .rows(6)
        .on_edit(move |s: &mut Ui| &mut s.memory.draft_mut(&tg).body)
        .build(&t.kit)
        .key("mem-create-body")
        .w_full();
    let blocked = d.busy || d.name.text.is_empty() || project.is_none();
    let tg = target.clone();
    let submit = Button::new(if d.busy {
        "Creating…"
    } else {
        "Create memory"
    })
    .control_size(ControlSize::Large)
    .disabled(blocked)
    .on_press(move |s: &mut Ui| s.submit_create(&tg))
    .build(&t.kit)
    .key("mem-create-submit");
    let tg = target.clone();
    let cancel = Button::new("Cancel")
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Large)
        .on_press(move |s: &mut Ui| s.memory.close_create(&tg))
        .build(&t.kit)
        .key("mem-create-cancel");
    body = body
        .child(field("Name (kebab-case)", name, t))
        .child(field("Description", description, t))
        .child(field("Type", kind, t))
        .child(field("Body (markdown)", text_body, t))
        .children(error_line("mem-create-error", d.error.as_ref()))
        .child(footer(cancel, submit));
    let tg = target.clone();
    KitDialog::new("mem-create", "New memory")
        .open(true)
        .fit()
        .width(WIDTH)
        .description(create_description(&d.name.text, label.as_deref()))
        .child(body)
        .on_close(move |s: &mut Ui| s.memory.close_create(&tg))
        .build(&t.kit, layer);
    layer.extend(popups);
}

fn conflict(f: &EditForm, t: &Theme) -> El {
    let reload = Button::new("Reload from disk")
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Large)
        .on_press(|s: &mut Ui| s.reload_from_disk())
        .build(&t.kit)
        .key("mem-cas-reload");
    let overwrite = Button::new("Overwrite")
        .variant(ButtonVariant::Danger)
        .control_size(ControlSize::Large)
        .disabled(f.busy())
        .on_press(|s: &mut Ui| s.submit_edit(false))
        .build(&t.kit)
        .key("mem-cas-overwrite");
    col()
        .key("mem-cas-conflict")
        .w_full()
        .gap(12.0)
        .radius(6.0)
        .border(1.0, amber_400().alpha(0.4))
        .fill(amber_400().alpha(0.1))
        .padding(16.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(icon(&glyphs::CIRCLE_X, 16.0).color(amber_300()))
                .child(
                    text("File changed on disk")
                        .font(ty::SM_MEDIUM)
                        .color(amber_300()),
                ),
        )
        .child(
            text(conflict_text(&f.entry.file_name))
                .font(ty::SM)
                .color(t.muted_fg)
                .wrap(),
        )
        .children(error_line("mem-edit-error", f.error.as_ref()))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(reload)
                .child(overwrite),
        )
}

fn edit(f: &EditForm, t: &Theme, layer: &mut Vec<El>) {
    let mut popups = Vec::new();
    let mut body = col().w_full().gap(12.0).px(16.0).pb(16.0);
    if f.conflict {
        body = body.child(conflict(f, t));
    } else {
        let description = TextInput::new(&f.description)
            .on_edit(|s: &mut Ui| s.memory.edit_text(false))
            .build(&t.kit)
            .key("mem-edit-description")
            .w_full();
        let kind = type_select(
            TypeField("mem-edit-type", &f.kind, f.kind_open),
            t,
            &mut popups,
            |s| {
                if let Some(f) = s.memory.edit_mut() {
                    f.kind_open = !f.kind_open;
                }
            },
            |s| {
                if let Some(f) = s.memory.edit_mut() {
                    f.kind_open = false;
                }
            },
            |s, k| {
                if let Some(f) = s.memory.edit_mut() {
                    k.clone_into(&mut f.kind);
                    f.kind_open = false;
                }
            },
        );
        let text_body = TextArea::new(&f.body)
            .rows(8)
            .on_edit(|s: &mut Ui| s.memory.edit_text(true))
            .build(&t.kit)
            .key("mem-edit-body")
            .w_full();
        let cancel = Button::new("Cancel")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Large)
            .on_press(|s: &mut Ui| s.memory.dialog = None)
            .build(&t.kit)
            .key("mem-edit-cancel");
        let label = if f.busy() {
            "Saving…"
        } else {
            "Save changes"
        };
        let submit = Button::new(label)
            .control_size(ControlSize::Large)
            .disabled(f.busy())
            .on_press(|s: &mut Ui| s.submit_edit(true))
            .build(&t.kit)
            .key("mem-edit-submit");
        body = body
            .child(field("Description", description, t))
            .child(field("Type", kind, t))
            .child(field("Body", text_body, t))
            .children(error_line("mem-edit-error", f.error.as_ref()))
            .child(footer(cancel, submit));
    }
    KitDialog::new("mem-edit", format!("Edit {}", f.entry.slug))
        .open(true)
        .fit()
        .width(WIDTH)
        .description("Re-serializes frontmatter + body and keeps the index line in sync.")
        .child(body)
        .on_close(|s: &mut Ui| s.memory.dialog = None)
        .build(&t.kit, layer);
    layer.extend(popups);
}

fn delete(f: &DeleteForm, t: &Theme, layer: &mut Vec<El>) {
    let cancel = Button::new("Cancel")
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Large)
        .on_press(|s: &mut Ui| s.memory.dialog = None)
        .build(&t.kit)
        .key("mem-delete-cancel");
    let confirm = Button::new(if f.busy() { "Deleting…" } else { "Delete" })
        .variant(ButtonVariant::Danger)
        .control_size(ControlSize::Large)
        .disabled(f.busy())
        .on_press(|s: &mut Ui| s.submit_delete())
        .build(&t.kit)
        .key("mem-delete-confirm");
    KitDialog::new("mem-delete", format!("Delete {}?", f.entry.slug))
        .open(true)
        .fit()
        .description(delete_description(&f.entry.slug, &f.entry.file_name))
        .child(
            col()
                .w_full()
                .px(16.0)
                .pb(16.0)
                .child(footer(cancel, confirm)),
        )
        .on_close(|s: &mut Ui| s.memory.dialog = None)
        .build(&t.kit, layer);
}

fn toasts(s: &Ui, t: &Theme) -> Option<El> {
    if s.memory.toasts.is_empty() {
        return None;
    }
    let region = s.memory.toasts.iter().fold(ToastRegion::new(), |r, toast| {
        let (glyph, color) = match toast.kind {
            ToastKind::Success => (&glyphs::CHECK, emerald_400()),
            ToastKind::Warning => (&glyphs::CIRCLE_X, amber_400()),
            ToastKind::Error => (&glyphs::CIRCLE_X, red_400()),
        };
        let id = toast.id;
        let mut b = Toast::new(format!("mem-toast:{id}"), toast.title.clone())
            .lead(icon(glyph, 16.0).color(color))
            .on_close(move |s: &mut Ui| s.memory.toasts.retain(|x| x.id != id));
        if let Some(d) = &toast.description {
            b = b.description(d.clone());
        }
        r.toast(b.build(&t.kit))
    });
    Some(region.build(&t.kit))
}

/// The open dialog and the toasts.
pub fn layer(s: &Ui, t: &Theme, layer: &mut Vec<El>) {
    match &s.memory.dialog {
        Some(Dialog::Create(target)) if s.memory.can_write() => create(s, target, t, layer),
        Some(Dialog::Edit(f)) if s.memory.can_write() => edit(f, t, layer),
        Some(Dialog::Delete(f)) if s.memory.can_write() => delete(f, t, layer),
        _ => {}
    }
    layer.extend(toasts(s, t));
}
