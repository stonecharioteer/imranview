use super::*;

impl ImranViewApp {
    pub(super) fn draw_resize_dialog(&mut self, ctx: &egui::Context) {
        if !self.resize_dialog.open {
            return;
        }

        let mut open = self.resize_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.resize",
            "Resize / Resample",
            egui::vec2(460.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                let aspect = if let Some((width, height)) = app.state.original_dimensions() {
                    if height > 0 {
                        width as f32 / height as f32
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };

                let before_width = app.resize_dialog.width;
                let before_height = app.resize_dialog.height;
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut app.resize_dialog.width).range(1..=65535));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut app.resize_dialog.height).range(1..=65535));
                });
                ui.checkbox(&mut app.resize_dialog.keep_aspect, "Keep aspect ratio");
                if app.resize_dialog.keep_aspect {
                    if app.resize_dialog.width != before_width && aspect > 0.0 {
                        app.resize_dialog.height =
                            ((app.resize_dialog.width as f32 / aspect).round().max(1.0)) as u32;
                    } else if app.resize_dialog.height != before_height && aspect > 0.0 {
                        app.resize_dialog.width =
                            ((app.resize_dialog.height as f32 * aspect).round().max(1.0)) as u32;
                    }
                }

                ui.separator();
                ui.label("Filter:");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Nearest,
                        "Nearest",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Triangle,
                        "Triangle",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::CatmullRom,
                        "CatmullRom",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Gaussian,
                        "Gaussian",
                    );
                    ui.selectable_value(
                        &mut app.resize_dialog.filter,
                        ResizeFilter::Lanczos3,
                        "Lanczos3",
                    );
                });

                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::Resize {
                        width: app.resize_dialog.width.max(1),
                        height: app.resize_dialog.height.max(1),
                        filter: app.resize_dialog.filter,
                    });
                    *open_state = false;
                }
            },
        );
        self.resize_dialog.open = open;
    }

    pub(super) fn draw_crop_dialog(&mut self, ctx: &egui::Context) {
        if !self.crop_dialog.open {
            return;
        }

        let mut open = self.crop_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.crop",
            "Crop",
            egui::vec2(420.0, 180.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.x).range(0..=65535));
                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.y).range(0..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.width).range(1..=65535));
                    ui.label("Height:");
                    ui.add(egui::DragValue::new(&mut app.crop_dialog.height).range(1..=65535));
                });

                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::Crop {
                        x: app.crop_dialog.x,
                        y: app.crop_dialog.y,
                        width: app.crop_dialog.width.max(1),
                        height: app.crop_dialog.height.max(1),
                    });
                    *open_state = false;
                }
            },
        );
        self.crop_dialog.open = open;
    }

    pub(super) fn draw_color_dialog(&mut self, ctx: &egui::Context) {
        if !self.color_dialog.open {
            return;
        }

        let mut open = self.color_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.color",
            "Color Corrections",
            egui::vec2(460.0, 260.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.brightness, -255..=255)
                        .text("Brightness"),
                );
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.contrast, -100.0..=100.0)
                        .text("Contrast"),
                );
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.gamma, 0.1..=5.0)
                        .text("Gamma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.color_dialog.saturation, 0.0..=3.0)
                        .text("Saturation")
                        .fixed_decimals(2),
                );
                ui.checkbox(&mut app.color_dialog.grayscale, "Grayscale");

                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::ColorAdjust(ColorAdjustParams {
                        brightness: app.color_dialog.brightness,
                        contrast: app.color_dialog.contrast,
                        gamma: app.color_dialog.gamma,
                        saturation: app.color_dialog.saturation,
                        grayscale: app.color_dialog.grayscale,
                    }));
                    *open_state = false;
                }
            },
        );
        self.color_dialog.open = open;
    }

    pub(super) fn draw_border_dialog(&mut self, ctx: &egui::Context) {
        if !self.border_dialog.open {
            return;
        }

        let mut open = self.border_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.border",
            "Add Border / Frame",
            egui::vec2(430.0, 250.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Left");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.left).range(0..=65535));
                    ui.label("Right");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.right).range(0..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Top");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.top).range(0..=65535));
                    ui.label("Bottom");
                    ui.add(egui::DragValue::new(&mut app.border_dialog.bottom).range(0..=65535));
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Border color");
                    ui.color_edit_button_srgba(&mut app.border_dialog.color);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.border_dialog.left == 0
                        && app.border_dialog.right == 0
                        && app.border_dialog.top == 0
                        && app.border_dialog.bottom == 0
                    {
                        app.state
                            .set_error("set at least one border side above zero");
                    } else {
                        app.dispatch_transform(TransformOp::AddBorder {
                            left: app.border_dialog.left,
                            right: app.border_dialog.right,
                            top: app.border_dialog.top,
                            bottom: app.border_dialog.bottom,
                            color: app.border_dialog.color.to_array(),
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.border_dialog.open = open;
    }

    pub(super) fn draw_canvas_dialog(&mut self, ctx: &egui::Context) {
        if !self.canvas_dialog.open {
            return;
        }

        let mut open = self.canvas_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.canvas",
            "Canvas Size",
            egui::vec2(520.0, 330.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(egui::DragValue::new(&mut app.canvas_dialog.width).range(1..=65535));
                    ui.label("Height");
                    ui.add(egui::DragValue::new(&mut app.canvas_dialog.height).range(1..=65535));
                });
                ui.separator();
                ui.label("Anchor");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::TopLeft,
                        "Top-left",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::TopCenter,
                        "Top-center",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::TopRight,
                        "Top-right",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::CenterLeft,
                        "Center-left",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::Center,
                        "Center",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::CenterRight,
                        "Center-right",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::BottomLeft,
                        "Bottom-left",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::BottomCenter,
                        "Bottom-center",
                    );
                    ui.selectable_value(
                        &mut app.canvas_dialog.anchor,
                        CanvasAnchor::BottomRight,
                        "Bottom-right",
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Fill color");
                    ui.color_edit_button_srgba(&mut app.canvas_dialog.fill);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.canvas_dialog.width == 0 || app.canvas_dialog.height == 0 {
                        app.state
                            .set_error("canvas dimensions must be greater than zero");
                    } else {
                        app.dispatch_transform(TransformOp::CanvasSize {
                            width: app.canvas_dialog.width,
                            height: app.canvas_dialog.height,
                            anchor: app.canvas_dialog.anchor,
                            fill: app.canvas_dialog.fill.to_array(),
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.canvas_dialog.open = open;
    }

    pub(super) fn draw_fine_rotate_dialog(&mut self, ctx: &egui::Context) {
        if !self.fine_rotate_dialog.open {
            return;
        }

        let mut open = self.fine_rotate_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.fine-rotate",
            "Fine Rotation",
            egui::vec2(470.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.add(
                    egui::Slider::new(&mut app.fine_rotate_dialog.angle_degrees, -180.0..=180.0)
                        .text("Angle (degrees)")
                        .fixed_decimals(2),
                );
                ui.horizontal(|ui| {
                    ui.label("Interpolation");
                    ui.selectable_value(
                        &mut app.fine_rotate_dialog.interpolation,
                        RotationInterpolation::Bilinear,
                        "Bilinear",
                    );
                    ui.selectable_value(
                        &mut app.fine_rotate_dialog.interpolation,
                        RotationInterpolation::Nearest,
                        "Nearest",
                    );
                });
                ui.checkbox(
                    &mut app.fine_rotate_dialog.expand_canvas,
                    "Expand canvas to fit",
                );
                ui.horizontal(|ui| {
                    ui.label("Background fill");
                    ui.color_edit_button_srgba(&mut app.fine_rotate_dialog.fill);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::RotateFine {
                        angle_degrees: app.fine_rotate_dialog.angle_degrees,
                        interpolation: app.fine_rotate_dialog.interpolation,
                        expand_canvas: app.fine_rotate_dialog.expand_canvas,
                        fill: app.fine_rotate_dialog.fill.to_array(),
                    });
                    *open_state = false;
                }
            },
        );
        self.fine_rotate_dialog.open = open;
    }

    pub(super) fn draw_text_tool_dialog(&mut self, ctx: &egui::Context) {
        if !self.text_tool_dialog.open {
            return;
        }

        let mut open = self.text_tool_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.text-tool",
            "Text Tool",
            egui::vec2(500.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.label("Text");
                ui.text_edit_multiline(&mut app.text_tool_dialog.text);
                ui.horizontal(|ui| {
                    ui.label("X");
                    ui.add(egui::DragValue::new(&mut app.text_tool_dialog.x).range(-65535..=65535));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut app.text_tool_dialog.y).range(-65535..=65535));
                });
                ui.horizontal(|ui| {
                    ui.label("Scale");
                    ui.add(egui::Slider::new(&mut app.text_tool_dialog.scale, 1..=16));
                    ui.label("Color");
                    ui.color_edit_button_srgba(&mut app.text_tool_dialog.color);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.text_tool_dialog.text.trim().is_empty() {
                        app.state.set_error("text content is required");
                    } else {
                        app.dispatch_transform(TransformOp::AddText {
                            text: app.text_tool_dialog.text.clone(),
                            x: app.text_tool_dialog.x,
                            y: app.text_tool_dialog.y,
                            scale: app.text_tool_dialog.scale,
                            color: app.text_tool_dialog.color.to_array(),
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.text_tool_dialog.open = open;
    }

    pub(super) fn draw_shape_tool_dialog(&mut self, ctx: &egui::Context) {
        if !self.shape_tool_dialog.open {
            return;
        }

        let mut open = self.shape_tool_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.shape-tool",
            "Shape Tool",
            egui::vec2(560.0, 320.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut app.shape_tool_dialog.kind, ShapeKind::Line, "Line");
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::Rectangle,
                        "Rectangle",
                    );
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::Ellipse,
                        "Ellipse",
                    );
                    ui.selectable_value(&mut app.shape_tool_dialog.kind, ShapeKind::Arrow, "Arrow");
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::RoundedRectangleShadow,
                        "Round Rect + Shadow",
                    );
                    ui.selectable_value(
                        &mut app.shape_tool_dialog.kind,
                        ShapeKind::SpeechBubble,
                        "Speech Bubble",
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Start X");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.start_x)
                            .range(-65535..=65535),
                    );
                    ui.label("Start Y");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.start_y)
                            .range(-65535..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("End X");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.end_x)
                            .range(-65535..=65535),
                    );
                    ui.label("End Y");
                    ui.add(
                        egui::DragValue::new(&mut app.shape_tool_dialog.end_y)
                            .range(-65535..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Thickness");
                    ui.add(egui::Slider::new(
                        &mut app.shape_tool_dialog.thickness,
                        1..=64,
                    ));
                    ui.checkbox(&mut app.shape_tool_dialog.filled, "Filled");
                });
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_srgba(&mut app.shape_tool_dialog.color);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::DrawShape(ShapeParams {
                        kind: app.shape_tool_dialog.kind,
                        start_x: app.shape_tool_dialog.start_x,
                        start_y: app.shape_tool_dialog.start_y,
                        end_x: app.shape_tool_dialog.end_x,
                        end_y: app.shape_tool_dialog.end_y,
                        thickness: app.shape_tool_dialog.thickness,
                        filled: app.shape_tool_dialog.filled,
                        color: app.shape_tool_dialog.color.to_array(),
                    }));
                    *open_state = false;
                }
            },
        );
        self.shape_tool_dialog.open = open;
    }

    pub(super) fn draw_overlay_dialog(&mut self, ctx: &egui::Context) {
        if !self.overlay_dialog.open {
            return;
        }

        let mut open = self.overlay_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.overlay",
            "Overlay / Watermark",
            egui::vec2(620.0, 280.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Overlay image");
                    ui.text_edit_singleline(&mut app.overlay_dialog.overlay_path);
                    if ui.button("Pick...").clicked() {
                        let dialog = rfd::FileDialog::new().set_title("Choose overlay image");
                        if let Some(path) = dialog.pick_file() {
                            app.overlay_dialog.overlay_path = path.display().to_string();
                        }
                    }
                });
                ui.add(
                    egui::Slider::new(&mut app.overlay_dialog.opacity, 0.0..=1.0)
                        .text("Opacity")
                        .fixed_decimals(2),
                );
                ui.label("Anchor");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::TopLeft,
                        "Top-left",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::TopCenter,
                        "Top-center",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::TopRight,
                        "Top-right",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::CenterLeft,
                        "Center-left",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::Center,
                        "Center",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::CenterRight,
                        "Center-right",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::BottomLeft,
                        "Bottom-left",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::BottomCenter,
                        "Bottom-center",
                    );
                    ui.selectable_value(
                        &mut app.overlay_dialog.anchor,
                        CanvasAnchor::BottomRight,
                        "Bottom-right",
                    );
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    let path = PathBuf::from(app.overlay_dialog.overlay_path.trim());
                    if path.as_os_str().is_empty() {
                        app.state.set_error("overlay image path is required");
                    } else {
                        app.dispatch_transform(TransformOp::OverlayImage {
                            overlay_path: path,
                            opacity: app.overlay_dialog.opacity,
                            anchor: app.overlay_dialog.anchor,
                        });
                        *open_state = false;
                    }
                }
            },
        );
        self.overlay_dialog.open = open;
    }

    pub(super) fn draw_selection_workflow_dialog(&mut self, ctx: &egui::Context) {
        if !self.selection_workflow_dialog.open {
            return;
        }

        let mut open = self.selection_workflow_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.selection-workflows",
            "Selection Workflows",
            egui::vec2(620.0, 340.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropRect,
                        "Crop Rect",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropCircle,
                        "Crop Circle",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CutOutsideRect,
                        "Cut Outside Rect",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CutOutsideCircle,
                        "Cut Outside Circle",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropPolygon,
                        "Crop Polygon",
                    );
                    ui.selectable_value(
                        &mut app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CutOutsidePolygon,
                        "Cut Outside Polygon",
                    );
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("X");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.x).range(0..=65535),
                    );
                    ui.label("Y");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.y).range(0..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Width");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.width)
                            .range(1..=65535),
                    );
                    ui.label("Height");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.height)
                            .range(1..=65535),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Radius");
                    ui.add(
                        egui::DragValue::new(&mut app.selection_workflow_dialog.radius)
                            .range(1..=65535),
                    );
                });
                if matches!(
                    app.selection_workflow_dialog.workflow,
                    SelectionWorkflow::CropPolygon | SelectionWorkflow::CutOutsidePolygon
                ) {
                    ui.label("Polygon points (x,y; x,y; x,y)");
                    ui.text_edit_singleline(&mut app.selection_workflow_dialog.polygon_points);
                }
                ui.horizontal(|ui| {
                    ui.label("Fill (for cut-outside)");
                    ui.color_edit_button_srgba(&mut app.selection_workflow_dialog.fill);
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    let polygon_points = if matches!(
                        app.selection_workflow_dialog.workflow,
                        SelectionWorkflow::CropPolygon | SelectionWorkflow::CutOutsidePolygon
                    ) {
                        match Self::parse_polygon_points(
                            &app.selection_workflow_dialog.polygon_points,
                        ) {
                            Some(points) => points,
                            None => {
                                app.state
                                    .set_error("invalid polygon points (expected: x,y; x,y; x,y)");
                                return;
                            }
                        }
                    } else {
                        Vec::new()
                    };
                    app.dispatch_transform(TransformOp::SelectionWorkflow(SelectionParams {
                        workflow: app.selection_workflow_dialog.workflow,
                        x: app.selection_workflow_dialog.x,
                        y: app.selection_workflow_dialog.y,
                        width: app.selection_workflow_dialog.width,
                        height: app.selection_workflow_dialog.height,
                        radius: app.selection_workflow_dialog.radius,
                        polygon_points,
                        fill: app.selection_workflow_dialog.fill.to_array(),
                    }));
                    *open_state = false;
                }
            },
        );
        self.selection_workflow_dialog.open = open;
    }

    pub(super) fn draw_replace_color_dialog(&mut self, ctx: &egui::Context) {
        if !self.replace_color_dialog.open {
            return;
        }

        let mut open = self.replace_color_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.replace-color",
            "Replace Color",
            egui::vec2(480.0, 260.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal(|ui| {
                    ui.label("Source");
                    ui.color_edit_button_srgba(&mut app.replace_color_dialog.source);
                    ui.label("Target");
                    ui.color_edit_button_srgba(&mut app.replace_color_dialog.target);
                });
                ui.add(
                    egui::Slider::new(&mut app.replace_color_dialog.tolerance, 0..=255)
                        .text("Tolerance"),
                );
                ui.checkbox(
                    &mut app.replace_color_dialog.preserve_alpha,
                    "Preserve original alpha",
                );
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::ReplaceColor {
                        source: app.replace_color_dialog.source.to_array(),
                        target: app.replace_color_dialog.target.to_array(),
                        tolerance: app.replace_color_dialog.tolerance,
                        preserve_alpha: app.replace_color_dialog.preserve_alpha,
                    });
                    *open_state = false;
                }
            },
        );
        self.replace_color_dialog.open = open;
    }

    pub(super) fn draw_alpha_dialog(&mut self, ctx: &egui::Context) {
        if !self.alpha_dialog.open {
            return;
        }

        let mut open = self.alpha_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.alpha",
            "Alpha Tools",
            egui::vec2(560.0, 340.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Mode");
                    ui.selectable_value(
                        &mut app.alpha_dialog.mode,
                        AlphaToolMode::Global,
                        "Global",
                    );
                    ui.selectable_value(
                        &mut app.alpha_dialog.mode,
                        AlphaToolMode::Brush,
                        "Brush dab",
                    );
                });
                ui.separator();
                if app.alpha_dialog.mode == AlphaToolMode::Global {
                    ui.add(
                        egui::Slider::new(&mut app.alpha_dialog.alpha_percent, 0.0..=100.0)
                            .text("Global alpha (%)")
                            .fixed_decimals(1),
                    );
                    ui.checkbox(
                        &mut app.alpha_dialog.alpha_from_luma,
                        "Derive alpha from luminance",
                    );
                    if app.alpha_dialog.alpha_from_luma {
                        ui.checkbox(&mut app.alpha_dialog.invert_luma, "Invert luminance alpha");
                    }
                    ui.checkbox(
                        &mut app.alpha_dialog.limit_to_region,
                        "Apply only to rectangular region",
                    );
                    if app.alpha_dialog.limit_to_region {
                        ui.horizontal(|ui| {
                            ui.label("X");
                            ui.add(
                                egui::DragValue::new(&mut app.alpha_dialog.region_x)
                                    .range(0..=1_000_000),
                            );
                            ui.label("Y");
                            ui.add(
                                egui::DragValue::new(&mut app.alpha_dialog.region_y)
                                    .range(0..=1_000_000),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label("Width");
                            ui.add(
                                egui::DragValue::new(&mut app.alpha_dialog.region_width)
                                    .range(1..=1_000_000),
                            );
                            ui.label("Height");
                            ui.add(
                                egui::DragValue::new(&mut app.alpha_dialog.region_height)
                                    .range(1..=1_000_000),
                            );
                        });
                    }
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Brush operation");
                        ui.selectable_value(
                            &mut app.alpha_dialog.brush_operation,
                            AlphaBrushOp::Decrease,
                            "Decrease alpha",
                        );
                        ui.selectable_value(
                            &mut app.alpha_dialog.brush_operation,
                            AlphaBrushOp::Increase,
                            "Increase alpha",
                        );
                        ui.selectable_value(
                            &mut app.alpha_dialog.brush_operation,
                            AlphaBrushOp::SetTransparent,
                            "Set transparent",
                        );
                        ui.selectable_value(
                            &mut app.alpha_dialog.brush_operation,
                            AlphaBrushOp::SetOpaque,
                            "Set opaque",
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Center X");
                        ui.add(
                            egui::DragValue::new(&mut app.alpha_dialog.brush_center_x)
                                .range(0..=1_000_000),
                        );
                        ui.label("Center Y");
                        ui.add(
                            egui::DragValue::new(&mut app.alpha_dialog.brush_center_y)
                                .range(0..=1_000_000),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Radius");
                        ui.add(
                            egui::DragValue::new(&mut app.alpha_dialog.brush_radius)
                                .range(1..=100_000),
                        );
                    });
                    ui.add(
                        egui::Slider::new(
                            &mut app.alpha_dialog.brush_strength_percent,
                            0.0..=100.0,
                        )
                        .text("Strength (%)")
                        .fixed_decimals(1),
                    );
                    ui.add(
                        egui::Slider::new(&mut app.alpha_dialog.brush_softness, 0.0..=1.0)
                            .text("Softness")
                            .fixed_decimals(2),
                    );
                }
                ui.separator();
                if ui.button("Apply").clicked() {
                    if app.alpha_dialog.mode == AlphaToolMode::Global {
                        app.dispatch_transform(TransformOp::AlphaAdjust {
                            alpha_percent: app.alpha_dialog.alpha_percent,
                            alpha_from_luma: app.alpha_dialog.alpha_from_luma,
                            invert_luma: app.alpha_dialog.invert_luma,
                            region: if app.alpha_dialog.limit_to_region {
                                Some((
                                    app.alpha_dialog.region_x,
                                    app.alpha_dialog.region_y,
                                    app.alpha_dialog.region_width.max(1),
                                    app.alpha_dialog.region_height.max(1),
                                ))
                            } else {
                                None
                            },
                        });
                    } else {
                        app.dispatch_transform(TransformOp::AlphaBrush {
                            center_x: app.alpha_dialog.brush_center_x,
                            center_y: app.alpha_dialog.brush_center_y,
                            radius: app.alpha_dialog.brush_radius.max(1),
                            strength_percent: app.alpha_dialog.brush_strength_percent,
                            softness: app.alpha_dialog.brush_softness,
                            operation: app.alpha_dialog.brush_operation,
                        });
                    }
                    *open_state = false;
                }
            },
        );
        self.alpha_dialog.open = open;
    }

    pub(super) fn draw_effects_dialog(&mut self, ctx: &egui::Context) {
        if !self.effects_dialog.open {
            return;
        }

        let mut open = self.effects_dialog.open;
        self.show_popup_window(
            ctx,
            "popup.effects",
            "Effects",
            egui::vec2(560.0, 360.0),
            &mut open,
            |app, _ctx, ui, open_state| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Presets");
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::Natural,
                            "Natural",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Natural);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::Vintage,
                            "Vintage",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Vintage);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::Dramatic,
                            "Dramatic",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Dramatic);
                    }
                    if ui
                        .selectable_label(app.effects_dialog.preset == EffectsPreset::Noir, "Noir")
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::Noir);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::StainedGlass,
                            "Stained Glass",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::StainedGlass);
                    }
                    if ui
                        .selectable_label(
                            app.effects_dialog.preset == EffectsPreset::TiltShift,
                            "Tilt Shift",
                        )
                        .clicked()
                    {
                        app.apply_effects_preset(EffectsPreset::TiltShift);
                    }
                    if ui.button("Reset").clicked() {
                        app.apply_effects_preset(EffectsPreset::Custom);
                        app.effects_dialog = EffectsDialogState::default();
                        app.effects_dialog.open = true;
                    }
                    if ui.button("Save preset...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Save effects preset (JSON)");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.set_file_name("effects-preset.json");
                        if let Some(path) = dialog.save_file() {
                            app.save_effects_preset(path);
                        }
                    }
                    if ui.button("Load preset...").clicked() {
                        let mut dialog =
                            rfd::FileDialog::new().set_title("Load effects preset (JSON)");
                        if let Some(directory) = app.state.preferred_open_directory() {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog = dialog.add_filter("JSON", &["json"]);
                        if let Some(path) = dialog.pick_file() {
                            app.load_effects_preset(path);
                        }
                    }
                });
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.blur_sigma, 0.0..=20.0)
                        .text("Blur sigma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.sharpen_sigma, 0.0..=20.0)
                        .text("Sharpen sigma")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.sharpen_threshold, -255..=255)
                        .text("Sharpen threshold"),
                );
                ui.checkbox(&mut app.effects_dialog.invert, "Invert");
                ui.checkbox(&mut app.effects_dialog.grayscale, "Grayscale");
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.sepia_strength, 0.0..=1.0)
                        .text("Sepia strength")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.posterize_levels, 0..=64)
                        .text("Posterize levels (0 = off)"),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.vignette_strength, 0.0..=1.0)
                        .text("Vignette")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.tilt_shift_strength, 0.0..=1.0)
                        .text("Tilt-shift")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.stained_glass_strength, 0.0..=1.0)
                        .text("Stained-glass")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.emboss_strength, 0.0..=1.0)
                        .text("Emboss")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.edge_enhance_strength, 0.0..=1.0)
                        .text("Edge enhance")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut app.effects_dialog.oil_paint_strength, 0.0..=1.0)
                        .text("Oil paint")
                        .fixed_decimals(2),
                );
                ui.separator();
                if ui.button("Apply").clicked() {
                    app.dispatch_transform(TransformOp::Effects(app.effects_params_from_dialog()));
                    *open_state = false;
                }
            },
        );
        self.effects_dialog.open = open;
    }
}
