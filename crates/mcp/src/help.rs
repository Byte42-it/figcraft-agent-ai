//! The guide the HTTP transport serves at `/help`: how an AI agent (Claude Code, Claude Desktop or
//! any MCP client, or plain HTTP) connects to a running FigCraft and designs with it. Built from the
//! same tool, prompt and resource definitions the server answers with, so it can't drift from them.

use serde_json::Value;

use crate::prompts::PROMPTS;
use crate::server::{INSTRUCTIONS, PROTOCOL_VERSION};
use crate::tools::tool_definitions;

/// The guide as Markdown, for a server whose address is `base` (`http://127.0.0.1:PORT`).
pub fn markdown(base: &str) -> String {
    let mcp = format!("{base}/mcp");
    let mut s = String::new();
    s.push_str(&format!(
        "# FigCraft MCP server\n\n\
FigCraft is running on this computer and lets AI agents design in it **as if they were the user**: \
create and open files, add pages, draw frames, shapes, text and vectors, use auto layout, components, \
styles, variables and prototypes, run any menu command, and look at the result. Everything an agent does \
happens live in the open FigCraft window and is one undo step the user can take back.\n\n\
- MCP endpoint (Streamable HTTP, protocol `{PROTOCOL_VERSION}`): **{mcp}**\n\
- This guide: {base}/help (Markdown) · tool schemas: {base}/tools (JSON) · status: {base}/health\n\
- Only this computer can connect (the server listens on 127.0.0.1 and refuses other `Host` and `Origin` headers). \
The port is chosen when the server starts in FigCraft (left sidebar ▸ MCP ▸ Start): if it changes, update the \
agent's configuration below.\n\n"
    ));

    s.push_str("## 1. Connect an agent\n\n");
    s.push_str(&format!(
        "### Claude Code (terminal)\n\n```sh\nclaude mcp add --transport http figcraft {mcp}\n```\n\n\
Then ask, for example: *\"Use the figcraft tools to design a login screen for a mobile app\"*. \
`claude mcp list` shows whether it is connected; `claude mcp remove figcraft` removes it.\n\n\
Or in a project's `.mcp.json`:\n\n```json\n{{\n  \"mcpServers\": {{\n    \"figcraft\": {{ \"type\": \"http\", \"url\": \"{mcp}\" }}\n  }}\n}}\n```\n\n"
    ));
    s.push_str(&format!(
        "### Claude Desktop\n\nClaude Desktop starts local servers as commands, so bridge to this URL with \
[`mcp-remote`](https://www.npmjs.com/package/mcp-remote) (needs Node.js). Settings ▸ Developer ▸ Edit Config \
(`claude_desktop_config.json`):\n\n```json\n{{\n  \"mcpServers\": {{\n    \"figcraft\": {{\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"mcp-remote\", \"{mcp}\"]\n    }}\n  }}\n}}\n```\n\n\
Restart Claude Desktop; FigCraft's tools then appear under the tools (hammer) menu. Without Node.js, \
build `figcraft-cli` and use `{{\"command\": \"/path/to/figcraft-cli\", \"args\": [\"mcp\", \"--attach\", \"PORT\"]}}` \
with FigCraft started as `figcraft --control PORT` (see docs/mcp.md).\n\n"
    ));
    s.push_str(&format!(
        "### Any other MCP client (Cursor, VS Code, Windsurf, Zed, custom agents…)\n\n\
Add a server of type **HTTP** / **Streamable HTTP** with the URL `{mcp}`. Most clients take:\n\n\
```json\n{{ \"mcpServers\": {{ \"figcraft\": {{ \"url\": \"{mcp}\" }} }} }}\n```\n\n\
### Plain HTTP (no MCP library)\n\nEach JSON-RPC 2.0 message is one `POST` to `/mcp` with a JSON body; the reply is \
the JSON-RPC response (`application/json`). Notifications get `202 Accepted` and no body.\n\n\
```sh\n\
curl -s {mcp} -H 'Content-Type: application/json' -H 'Accept: application/json, text/event-stream' \\\n  \
-d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"{PROTOCOL_VERSION}\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"curl\",\"version\":\"1\"}}}}}}'\n\
curl -s {mcp} -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}'\n\
curl -s {mcp} -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}'\n\
curl -s {mcp} -H 'Content-Type: application/json' \\\n  \
-d '{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"create_node\",\"arguments\":{{\"type\":\"rectangle\",\"x\":0,\"y\":0,\"width\":200,\"height\":120,\"props\":{{\"fill\":\"#7048E8\",\"cornerRadius\":16}}}}}}}}'\n\
```\n\n\
The server keeps no per-client session: every client drives the same FigCraft window, one request at a time.\n\n"
    ));

    s.push_str("## 2. How FigCraft works (read this first)\n\n");
    s.push_str(INSTRUCTIONS);
    s.push_str(
        "\n\n\
- **Everything is a command.** Every menu item, panel button and shortcut has a command id. `list_commands \
{\"filter\": \"layout\"}` finds them (id, label, menu, shortcut and parameters); `run_command` runs one; \
`batch` runs several as one undo step, and a step can use an earlier step's result (`{\"$step\": 0}` is the \
id step 0 returned).\n\
- **Act like the user.** `pointer` clicks and drags on the canvas with the current tool (`run_command tool.set \
{\"tool\": \"rectangle\"}` first), `key` presses shortcuts, `type_text` types into the text being edited. \
Prefer `create_node` / `set_properties` / `run_command` for precise work and the gestures when you need what a \
person's mouse would do.\n\
- **Nothing opens a dialog.** Commands that would ask the user something take parameters instead (paths, names).\n\
- **Look at your work.** `screenshot` returns a PNG of the page or of layers (`window: true`: the whole app \
window as the user sees it); `inspect_document` and `get_node` return exact numbers.\n\n",
    );

    s.push_str("## 3. Recipes\n\n");
    s.push_str(RECIPES);

    s.push_str("\n## 4. Tools\n\n");
    for t in tool_definitions() {
        let name = t.get("name").and_then(Value::as_str).unwrap_or_default();
        let title = t.get("title").and_then(Value::as_str).unwrap_or_default();
        let desc = t.get("description").and_then(Value::as_str).unwrap_or_default();
        let params: Vec<&str> =
            t.pointer("/inputSchema/properties").and_then(Value::as_object).map(|p| p.keys().map(String::as_str).collect()).unwrap_or_default();
        s.push_str(&format!("### `{name}` — {title}\n\n{desc}\n\n"));
        if !params.is_empty() {
            s.push_str(&format!("Arguments: {}\n\n", params.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", ")));
        }
    }
    s.push_str(&format!("Full JSON Schemas: {base}/tools, or the MCP `tools/list` method.\n\n"));

    s.push_str("## 5. Prompts (ready-made workflows, `prompts/get`)\n\n");
    for p in PROMPTS {
        let args: Vec<String> = p.args.iter().map(|a| if a.required { format!("`{}`", a.name) } else { format!("`{}?`", a.name) }).collect();
        s.push_str(&format!(
            "- **`{}`** — {} {}\n",
            p.name,
            p.description,
            if args.is_empty() { String::new() } else { format!("Arguments: {}.", args.join(", ")) }
        ));
    }
    s.push_str(
        "\n## 6. Resources (`resources/read`)\n\n\
- `figcraft://document`: the active document as JSON\n\
- `figcraft://commands`: every command with its parameters\n\
- `figcraft://node/{id}`: one layer and its children\n\
- `figcraft://command/{id}`: one command, e.g. `figcraft://command/layout.add`\n\n\
## 7. Good habits\n\n\
1. Start with `inspect_ui` and `inspect_document {\"depth\": 1}` to see which file, page and selection are open.\n\
2. Make a new file (`run_command file.new`) or start from a template instead of drawing over the user's work, \
unless asked to edit it.\n\
3. Build with frames and auto layout, give layers meaningful names, keep spacing on an 8-point grid.\n\
4. Check each step (`get_node`), take a `screenshot` at the end and fix what looks wrong.\n\
5. Save only when asked (`save_file {\"path\": \"…/name.figcraft\"}`); `undo` takes back a mistake.\n",
    );
    s
}

/// Worked examples, with command ids and parameters that exist.
const RECIPES: &str = "\
**Projects (files and pages)**\n\n\
- New empty file: `run_command {\"command\": \"file.new\"}`. From a starter template: `run_command \
{\"command\": \"template.list\"}`, then `run_command {\"command\": \"file.newFromTemplate\", \"params\": \
{\"template\": \"mobile-app\"}}` (also `landing-page`, `dashboard`, `design-system`, `wireframe-kit`, \
`social-posts`, `slides`, `icon-set`).\n\
- Open and save: `open_file {\"path\": \"/path/design.figcraft\"}` (also `.fig`, `.svg`, images, FigCraft JSON), \
`save_file {\"path\": \"/path/design.figcraft\"}` (no path: save in place).\n\
- Open files: `run_command doc.list` lists them, `run_command doc.switch {\"index\": 1}` brings one to the front, \
`run_command file.close` closes the active one.\n\
- Pages: `page.add {\"name\": \"Mobile\"}`, `page.list`, `page.select {\"id\": …}`, `page.rename`, `page.duplicate`, \
`page.delete`.\n\n\
**Drawing**\n\n\
- A screen: `run_command frame.create {\"preset\": \"iPhone 16\", \"x\": 0, \"y\": 0, \"name\": \"Login\"}` \
(`run_command frame.presets` lists devices), or `create_node {\"type\": \"frame\", \"x\": 0, \"y\": 0, \"width\": 1440, \
\"height\": 1024}`.\n\
- Shapes and text in it: `create_node {\"type\": \"rectangle\" | \"ellipse\" | \"polygon\" | \"star\" | \"line\" | \
\"arrow\" | \"text\" | \"vector\", \"parent\": <frame id>, \"x\": 24, \"y\": 24, \"width\": 200, \"height\": 48, \"text\": \
\"Hello\", \"props\": {\"fill\": \"#1E1E1E\", \"fontSize\": 24, \"fontStyle\": \"Bold\"}}`.\n\
- Vectors: `create_node {\"type\": \"vector\", \"path\": \"M0 0 L100 0 L50 80 Z\"}`; edit points with \
`vector.edit.begin` and the `vector.edit.*` commands.\n\
- Like a person with the mouse: `run_command tool.set {\"tool\": \"ellipse\"}` then `pointer {\"kind\": \"drag\", \
\"x\": 100, \"y\": 100, \"to\": [300, 260], \"shift\": true}`; the pen and pencil tools draw the same way.\n\
- Change anything: `set_properties {\"ids\": [..], \"props\": {\"fill\": \"#FF5733\", \"cornerRadius\": 12, \"opacity\": \
0.8, \"effects\": [{\"type\": \"dropShadow\", \"color\": \"#00000033\", \"offset\": [0, 4], \"radius\": 12}]}}`.\n\
- Arrange: `align.left`, `align.hcenter`, `distribute.horizontal`, `object.group`, `object.frameSelection`, \
`arrange.front`… (`list_commands {\"filter\": \"align\"}`).\n\n\
**Auto layout**\n\n\
- `set_properties {\"ids\": [<frame>], \"props\": {\"layout\": \"vertical\", \"gap\": 16, \"padding\": [24, 24, 24, 24], \
\"mainAlign\": \"min\", \"crossAlign\": \"center\", \"sizingV\": \"hug\"}}`; children take `sizingH: \"fill\"` to stretch. \
`layout.add` wraps the selection in an auto layout frame. Grids: `\"layout\": \"grid\", \"gridColumns\": 3`.\n\n\
**Components, styles and variables**\n\n\
- `component.create` on a frame makes a component; `instance.create {\"component\": <id>, \"x\": …, \"y\": …}` places \
instances; `component.combineVariants` makes a variant set; `instance.setProperties` switches variants.\n\
- Styles: `style.create {\"type\": \"paint\" | \"text\" | \"effect\" | \"grid\", \"name\": \"Brand/Primary\", \"from\": <id>}`, \
`style.apply {\"style\": <id>, \"ids\": [..]}`.\n\
- Variables: `variable.createCollection {\"name\": \"Theme\", \"modes\": [\"Light\", \"Dark\"]}`, `variable.create`, \
`variable.bind {\"ids\": [..], \"field\": \"fill\", \"variable\": <id>}`.\n\n\
**Prototypes**\n\n\
- `proto.interaction.add {\"id\": <button>, \"trigger\": {\"type\": \"onClick\"}, \"action\": {\"type\": \"navigate\", \
\"dest\": <frame>}, \"transition\": {\"type\": \"smartAnimate\"}, \"durationMs\": 300}`, `proto.flow.add {\"start\": \
<frame>, \"name\": \"Main\"}`, `proto.present` plays it in the window.\n\n\
**Inspect and export**\n\n\
- `screenshot {\"ids\": [<frame>]}` to look; `export {\"ids\": [..], \"format\": \"png\" | \"jpg\" | \"svg\" | \"pdf\", \"scale\": 2, \
\"path\": \"/path/out.png\"}`; `run_command inspect.code {\"id\": <frame>, \"language\": \"html\" | \"css\" | \"react\" | \
\"swiftui\" | \"compose\"}` for code.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_names_the_endpoint_every_tool_and_prompt() {
        let md = markdown("http://127.0.0.1:4242");
        assert!(md.contains("claude mcp add --transport http figcraft http://127.0.0.1:4242/mcp"));
        assert!(md.contains("\"mcp-remote\", \"http://127.0.0.1:4242/mcp\""));
        for t in crate::TOOL_NAMES {
            assert!(md.contains(&format!("### `{t}`")), "the guide lists {t}");
        }
        for p in PROMPTS {
            assert!(md.contains(p.name), "the guide lists prompt {}", p.name);
        }
    }

    #[test]
    fn the_recipes_name_only_commands_that_exist() {
        // Words like `file.new` in backticks or after `run_command`.
        let md = markdown("http://127.0.0.1:1");
        let (mut checked, mut missing) = (0, Vec::new());
        for word in md.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_')) {
            let Some((head, tail)) = word.split_once('.') else { continue };
            let families = [
                "file",
                "doc",
                "page",
                "frame",
                "tool",
                "vector",
                "align",
                "distribute",
                "object",
                "arrange",
                "layout",
                "component",
                "instance",
                "style",
                "variable",
                "proto",
                "inspect",
                "template",
            ];
            // `vector.edit.*` is a family, not a command.
            if !families.contains(&head) || tail.is_empty() || tail.ends_with('.') || word == "vector.edit" {
                continue;
            }
            if figcraft_engine::find_command(word).is_none() {
                missing.push(word.to_string());
            }
            checked += 1;
        }
        assert!(missing.is_empty(), "the guide names commands that don't exist: {missing:?}");
        assert!(checked > 20, "checked {checked} command names");
    }
}
