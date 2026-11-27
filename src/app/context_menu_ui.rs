use super::*;

fn header(text: impl Into<SharedString>) -> FloatingMenuEntry {
    FloatingMenuEntry {
        kind: "header".into(),
        id: "".into(),
        text: text.into(),
        shortcut: "".into(),
        icon_key: "".into(),
        enabled: false,
        danger: false,
    }
}

fn divider() -> FloatingMenuEntry {
    FloatingMenuEntry {
        kind: "divider".into(),
        id: "".into(),
        text: "".into(),
        shortcut: "".into(),
        icon_key: "".into(),
        enabled: false,
        danger: false,
    }
}

fn item(
    id: &str,
    text: &str,
    shortcut: &str,
    icon_key: &str,
    enabled: bool,
    danger: bool,
) -> FloatingMenuEntry {
    FloatingMenuEntry {
        kind: "item".into(),
        id: id.into(),
        text: text.into(),
        shortcut: shortcut.into(),
        icon_key: icon_key.into(),
        enabled,
        danger,
    }
}

/// Detecta se o backend de janelas permite posicionar janelas flutuantes em coordenadas globais.
/// Em X11/XWayland, Windows e macOS, o menu flutuante sem borda renderiza livremente fora da janela principal.
pub(super) fn supports_floating_context_menus() -> bool {
    #[cfg(target_os = "linux")]
    {
        let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let forced_x11 = std::env::var("WINIT_UNIX_BACKEND")
            .map(|v| v.eq_ignore_ascii_case("x11"))
            .unwrap_or(false);
        !on_wayland || forced_x11
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

fn position_menu_at(menu: &ContextMenuWindow, parent: &MainWindow, local_x: f32, local_y: f32) {
    let scale = parent.window().scale_factor().max(1.0);
    let p_pos = parent.window().position();
    // Em X11 `outer_position()` aponta para o canto externo (acima da barra de título ~28px);
    // somamos um pequeno deslocamento vertical aproximado apenas quando a janela tem decoração.
    let gx = p_pos.x + (local_x * scale).round() as i32;
    let gy = p_pos.y + (local_y * scale).round() as i32 + (28.0 * scale).round() as i32;
    menu.window()
        .set_position(slint::PhysicalPosition::new(gx.max(0), gy.max(0)));
}

pub(super) fn wire(main_window: &MainWindow, menu_window: &Rc<ContextMenuWindow>) {
    main_window.set_use_floating_context_menus(supports_floating_context_menus());

    let group_ctx: Rc<std::cell::RefCell<(String, String)>> =
        Rc::new(std::cell::RefCell::new((String::new(), String::new())));

    {
        let mw = menu_window.as_weak();
        menu_window.on_dismissed(move || {
            if let Some(w) = mw.upgrade() {
                let _ = w.hide();
            }
        });
    }
    {
        let mw = menu_window.as_weak();
        main_window.on_dismiss_floating_menu(move || {
            if let Some(w) = mw.upgrade() {
                let _ = w.hide();
            }
        });
    }

    // Dispatch de todas as ações do menu flutuante de volta aos callbacks da MainWindow
    {
        let main_weak = main_window.as_weak();
        let menu_weak = menu_window.as_weak();
        let group_ctx = Rc::clone(&group_ctx);
        menu_window.on_item_selected(move |action_id| {
            if let Some(mw) = menu_weak.upgrade() {
                let _ = mw.hide();
            }
            let Some(main) = main_weak.upgrade() else {
                return;
            };
            let inst_id = main.get_selected_instance_id();
            let inst_name = main.get_selected_instance_name();
            match action_id.as_str() {
                "inst:rename" => main.invoke_open_rename_instance_clicked(inst_id),
                "inst:icon" => main.invoke_change_icon_clicked(inst_id),
                "inst:launch" => main.invoke_launch_instance_clicked(inst_id),
                "inst:stop" => main.invoke_stop_instance_clicked(inst_id),
                "inst:edit" => main.invoke_edit_instance_clicked(inst_id),
                "inst:summary" => main.invoke_view_launch_config_clicked(inst_id),
                "inst:group" => main.invoke_change_group_clicked(inst_id),
                "inst:folder" => main.invoke_open_folder_clicked(inst_id),
                "inst:copy" => {
                    main.set_copy_new_name(SharedString::from(format!("{} Copy", inst_name)));
                    main.set_show_copy_dialog(true);
                }
                "inst:repair" => main.invoke_repair_instance_clicked(inst_id),
                "inst:delete" => main.invoke_request_delete_instance_dialog(inst_id, inst_name),
                "grp:rename" => {
                    let (gid, gname) = group_ctx.borrow().clone();
                    main.invoke_request_group_dialog(
                        "Rename Group".into(),
                        gid.into(),
                        gname.into(),
                    );
                }
                "grp:toggle" => {
                    let (gid, _) = group_ctx.borrow().clone();
                    main.invoke_toggle_group_collapsed(gid.into());
                }
                "grp:up" => {
                    let (gid, _) = group_ctx.borrow().clone();
                    main.invoke_move_group_up(gid.into());
                }
                "grp:down" => {
                    let (gid, _) = group_ctx.borrow().clone();
                    main.invoke_move_group_down(gid.into());
                }
                "grp:delete" => {
                    let (gid, gname) = group_ctx.borrow().clone();
                    main.invoke_request_delete_group_dialog(gid.into(), gname.into());
                }
                "empty:create_inst" => main.invoke_add_instance_clicked(),
                "empty:create_group" => {
                    main.invoke_request_group_dialog("Create Group".into(), "".into(), "".into())
                }
                _ => {}
            }
        });
    }

    // Menu de instância
    {
        let main_weak = main_window.as_weak();
        let menu_weak = menu_window.as_weak();
        main_window.on_open_floating_instance_menu(move |mx, my| {
            let (Some(main), Some(menu)) = (main_weak.upgrade(), menu_weak.upgrade()) else {
                return;
            };
            let running = main.get_selected_instance_running();
            let installing = main.get_selected_instance_status_code() == "installing";
            let entries = vec![
                header(main.get_selected_instance_name()),
                divider(),
                item("inst:rename", "Rename", "F2", "edit", true, false),
                item("inst:icon", "Change Icon", "", "cube", true, false),
                divider(),
                item(
                    "inst:launch",
                    "Launch",
                    "Enter",
                    "play",
                    !running && !installing,
                    false,
                ),
                item("inst:stop", "Stop Game", "Ctrl+K", "square", running, false),
                divider(),
                item(
                    "inst:edit",
                    "Edit Instance",
                    "Ctrl+I",
                    "settings",
                    true,
                    false,
                ),
                item(
                    "inst:summary",
                    "View Launch Configuration",
                    "",
                    "info",
                    true,
                    false,
                ),
                item(
                    "inst:group",
                    "Change Group",
                    "Ctrl+G",
                    "folder-plus",
                    true,
                    false,
                ),
                item("inst:folder", "Open Folder", "", "folder", true, false),
                divider(),
                item("inst:copy", "Copy Instance", "Ctrl+D", "copy", true, false),
                item(
                    "inst:repair",
                    "Repair Instance",
                    "",
                    "wrench",
                    !running && !installing,
                    false,
                ),
                item(
                    "inst:delete",
                    "Delete Instance",
                    "Del",
                    "trash",
                    !running,
                    true,
                ),
            ];
            menu.set_menu_width(244.0);
            menu.set_entries(ModelRc::from(Rc::new(VecModel::from(entries))));
            position_menu_at(&menu, &main, mx, my);
            let _ = menu.show();
        });
    }

    // Menu de grupo
    {
        let main_weak = main_window.as_weak();
        let menu_weak = menu_window.as_weak();
        let group_ctx = Rc::clone(&group_ctx);
        main_window.on_open_floating_group_menu(move |gid, gname, collapsed, mx, my| {
            let (Some(main), Some(menu)) = (main_weak.upgrade(), menu_weak.upgrade()) else {
                return;
            };
            *group_ctx.borrow_mut() = (gid.to_string(), gname.to_string());
            let entries = vec![
                header(gname),
                divider(),
                item("grp:rename", "Rename Group", "", "edit", true, false),
                item(
                    "grp:toggle",
                    if collapsed {
                        "Expand Group"
                    } else {
                        "Collapse Group"
                    },
                    "",
                    if collapsed {
                        "chevron-down"
                    } else {
                        "chevron-up"
                    },
                    true,
                    false,
                ),
                divider(),
                item("grp:up", "Move Group Up", "", "arrow-up", true, false),
                item("grp:down", "Move Group Down", "", "arrow-down", true, false),
                divider(),
                item("grp:delete", "Delete Group", "", "trash", true, true),
            ];
            menu.set_menu_width(208.0);
            menu.set_entries(ModelRc::from(Rc::new(VecModel::from(entries))));
            position_menu_at(&menu, &main, mx, my);
            let _ = menu.show();
        });
    }

    // Menu de área vazia
    {
        let main_weak = main_window.as_weak();
        let menu_weak = menu_window.as_weak();
        main_window.on_open_floating_empty_menu(move |mx, my| {
            let (Some(main), Some(menu)) = (main_weak.upgrade(), menu_weak.upgrade()) else {
                return;
            };
            let entries = vec![
                header("Crab Launcher"),
                divider(),
                item(
                    "empty:create_inst",
                    "Create Instance",
                    "",
                    "plus",
                    true,
                    false,
                ),
                item(
                    "empty:create_group",
                    "Create Group",
                    "",
                    "folder-plus",
                    true,
                    false,
                ),
            ];
            menu.set_menu_width(196.0);
            menu.set_entries(ModelRc::from(Rc::new(VecModel::from(entries))));
            position_menu_at(&menu, &main, mx, my);
            let _ = menu.show();
        });
    }
}
