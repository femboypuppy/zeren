-- Compact sidebar: replaces Zeren's whole sidebar module with one of its
-- own (sidebar.luau). A screen replacement runs as part of Zeren, which is
-- why it asks "ui.replace", and only for the module it names.
ui.override { module = "@app/screens/shell/sidebar", with = "./sidebar" }
