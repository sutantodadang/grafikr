use grafikr::{App, Signal, Theme, TreeNode, View, bind, column, text, tree_view};

fn demo() -> impl View {
    let selected = Signal::new(0usize);
    let status = selected.clone();
    let pick = selected.clone();

    column((
        text("TreeView").size(26.0),
        text(bind(move || format!("Node terpilih: {}", status.get()))).size(12.0),
        tree_view([
            TreeNode::new("src").child(TreeNode::new("main.rs")).child(
                TreeNode::new("widgets")
                    .child(TreeNode::new("button.rs"))
                    .child(TreeNode::new("text_field.rs")),
            ),
            TreeNode::new("docs")
                .child(TreeNode::new("ROADMAP.md"))
                .child(TreeNode::new("STATUS.md")),
            TreeNode::new("Cargo.toml"),
        ])
        .width(340.0)
        .height(320.0)
        .on_select(move |id| pick.set(id)),
    ))
    .padding(24.0)
    .gap(12.0)
}

fn main() {
    App::new(demo)
        .title("Grafikr — TreeView")
        .theme(Theme::dark())
        .size(520.0, 460.0)
        .run()
        .expect("event loop");
}
