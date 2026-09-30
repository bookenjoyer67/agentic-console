//! `--dump`: the whole reading as plain text, so the data layer can be checked in a pipe.

use crate::config::Config;
use crate::iso;
use crate::state::Snapshot;

/// Render a snapshot as plain text.
pub fn render(config: &Config, snapshot: &Snapshot) -> String {
    let mut out = String::new();
    out.push_str("agentic-console --dump\n");
    out.push_str("=======================\n");
    out.push_str(&format!("repo        : {}\n", config.repo.display()));
    out.push_str(&format!("config      : {}\n", config.source_line()));
    out.push_str(&format!(
        "config read : {} (age {})\n",
        iso::format_utc(config.read_at),
        iso::format_age(config.read_at, snapshot.read_at)
    ));
    out.push_str(&format!("container   : {}\n", config.console.container));
    out.push_str(&format!(
        "claude      : {}   workspace {}   gate port {} storage {} retrieval {}\n",
        config.console.claude_command,
        config.workspace(),
        config.console.gate_port,
        config.console.storage_port,
        config.console.retrieval_port
    ));
    out.push_str(&format!(
        "evidence dir: {}   (the harness's own run* copies; a transcript there is attributed to a\n\
         \x20             session only when its own file name carries that session's id)\n",
        config.console.evidence_dir.display()
    ));
    out.push_str(&format!(
        "briefs dir  : {}\n",
        config.console.briefs_dir.display()
    ));
    out.push_str(&format!(
        "gates       : {}\n",
        config.gate_names().join(", ")
    ));
    out.push_str(&format!("roles       : {}\n", config.roles().join(", ")));
    // The canned rulings, in the order the chooser numbers them: the wording `Enter` and the
    // chooser send, from the config, so a fork reads its own sentences back here before it presses
    // anything.
    out.push_str(&format!(
        "rulings     : {} in console.rulings; the first is the default `Enter` sends on LIVE, `e` \
         opens the chooser over all of them\n",
        config.rulings().len()
    ));
    for (index, ruling) in config.rulings().iter().enumerate() {
        out.push_str(&format!(
            "  {} {:<26} id {:<20} {}\n",
            index + 1,
            ruling.label,
            ruling.id,
            if ruling.prefill {
                "prefilled text input, then the confirmation"
            } else {
                "sent as written, straight to the confirmation"
            }
        ));
        out.push_str(&format!("      text: {}\n", ruling.text));
    }
    out.push_str(
        "  c free text                 (the chooser's own row: the empty box, as `e` always opened)\n",
    );
    out.push_str(&format!(
        "snapshot    : taken {}\n",
        iso::format_utc(snapshot.read_at)
    ));
    out.push_str(
        "every value below names the file, docker call or command it came from, with the age of the\n\
         reading. Nothing here is computed for display; a missing reading shows as missing.\n",
    );
    for warning in &snapshot.warnings {
        out.push_str(&format!("WARNING     : {warning}\n"));
    }
    render_flow(&mut out, snapshot);
    render_live(&mut out, snapshot);
    render_inspect(&mut out, snapshot);
    out.push_str("\n== ACTIONS (commands this console would run; see --dry-run-actions) ==\n");
    out.push_str(&crate::actions::dry_run_all(config));
    out
}

fn render_flow(out: &mut String, snapshot: &Snapshot) {
    out.push_str("\n== FLOW: the map with live lights ==\n");
    for lane in &snapshot.flow.lanes {
        out.push_str(&format!("\nLANE {} -- {}\n", lane.key, lane.title));
        out.push_str(&format!("  ({})\n", lane.blurb));
        for node in &lane.nodes {
            out.push_str(&format!(
                "  {} {:<10} {:<52} {}\n",
                node.light.glyph(),
                node.light.word(),
                node.label,
                node.status
            ));
            out.push_str(&format!("        artifact : {}\n", node.artifact));
            for line in &node.provenance {
                out.push_str(&format!("        source   : {line}\n"));
            }
        }
        for note in &lane.footnote {
            out.push_str(&format!("  note: {note}\n"));
        }
    }
}

fn render_live(out: &mut String, snapshot: &Snapshot) {
    let live = &snapshot.live;
    out.push_str("\n== LIVE: what is happening right now ==\n");
    out.push_str(&format!(
        "  run in flight : {}   container {} ({})   source {} ({})\n",
        live.run.in_flight,
        live.run.container,
        if live.run.container_running {
            "running"
        } else {
            "not running"
        },
        live.run.source,
        live.run.source_age
    ));
    if live.run.in_flight {
        out.push_str(&format!(
            "  process       : pid {} elapsed {} mode {}\n",
            live.run.pid, live.run.elapsed, live.run.mode
        ));
        out.push_str(&format!(
            "  brief         : {} characters in the -p argument\n",
            live.run.prompt_chars
        ));
        out.push_str(&format!(
            "  checkpoint    : {}\n",
            live.run
                .checkpoint_hint
                .clone()
                .unwrap_or_else(|| "none named in the brief".to_string())
        ));
    }
    out.push_str(&format!("  ports         : {}\n", live.port_line));
    match &live.gate_allowlist_error {
        Some(error) => out.push_str(&format!(
            "  gate server   : could not be asked ({}): {error}\n",
            live.gate_allowlist_source
        )),
        None => out.push_str(&format!(
            "  gate server   : list_gates -> {} ({}, {})\n",
            live.gate_allowlist.join(", "),
            live.gate_allowlist_source,
            live.gate_allowlist_age
        )),
    }
    out.push_str(&format!(
        "  container ps  : {} process(es) read from `{}` ({})\n",
        live.proc_total, live.proc_source, live.proc_age
    ));
    for row in &live.procs {
        out.push_str(&format!(
            "    pid {:<8} elapsed {:<12} {}{}\n",
            row.pid,
            row.elapsed,
            row.command_head,
            if row.is_agent {
                "   <- the agent CLI"
            } else {
                ""
            }
        ));
    }
    if let Some(error) = &live.run.error {
        out.push_str(&format!("  process error : {error}\n"));
    }
    out.push_str(&format!(
        "  gate journal  : {} ({}), {} lines total, last 20 shown\n",
        live.gate_source, live.gate_age, live.gate_total
    ));
    out.push_str("    gate           exit role           dur s  guard                timestamp\n");
    for entry in &live.gates {
        out.push_str(&format!(
            "    {:<14} {:>4} {:<14} {:>6} {:<20} {}\n",
            entry.gate,
            entry.exit_code,
            entry.role,
            format!("{:.1}", entry.duration),
            entry.guard_text(),
            entry.timestamp
        ));
        out.push_str(&format!("        argv: {}\n", entry.argv_text()));
    }
    out.push_str(&format!(
        "  storage journal: {} ({})\n",
        live.store_source, live.store_age
    ));
    for entry in &live.stores {
        out.push_str(&format!(
            "    {:<14} {:<14} {:<10} {:<10} {}\n",
            entry.role,
            entry.operation,
            entry.short_id(),
            entry.classification,
            entry.timestamp
        ));
    }
    out.push_str(&format!(
        "  retrieval journal: {} ({})\n",
        live.retrieval_source, live.retrieval_age
    ));
    for entry in &live.retrievals {
        out.push_str(&format!(
            "    {:<14} {:<9} {:<9} results {:<3} {}\n",
            entry.role, entry.ceiling, entry.decision, entry.result_count, entry.timestamp
        ));
    }
    out.push_str("\n  the three repository checks, last known result:\n");
    for check in &live.checks {
        out.push_str(&format!(
            "    {} {:<30} {}\n",
            check.light.glyph(),
            check.name,
            check.last
        ));
        out.push_str(&format!(
            "        source: {} ({})\n",
            check.source, check.age
        ));
    }
    out.push_str("\n  CHECKPOINT CARD\n");
    out.push_str(&format!("    state      : {}\n", live.checkpoint.word()));
    // The state's own name and what it rests on, so a pipe reads the same honest state the card
    // and the status bar carry.
    out.push_str(&format!(
        "    basis      : {}\n",
        live.checkpoint.state.basis_line()
    ));
    out.push_str(&format!("    checkpoint : {}\n", live.checkpoint.which));
    out.push_str(&format!("    decision   : {}\n", live.checkpoint.question));
    for line in &live.checkpoint.evidence {
        out.push_str(&format!("    evidence   : {line}\n"));
    }
    // The two reads the card's attribution rests on, each with its own source and age: the
    // session's own transcript in the container, and the evidence directory it was attributed
    // from. Anything the card quotes is one of these, and nothing here is a newer file by itself.
    out.push_str(&format!(
        "    transcript : {}\n",
        live.checkpoint.transcript_read
    ));
    out.push_str(&format!(
        "    evidence dir: {}\n",
        live.checkpoint.evidence_dir_read
    ));
    if let Some(conflict) = &live.checkpoint.checkpoint_conflict {
        out.push_str(&format!("    contested  : {conflict}\n"));
    }
    // Which session a ruling resumes, with the read that named it: the card's own line, plus
    // every note about whether the two reads agree. A refusal also names the session it refused
    // to resume.
    out.push_str(&format!(
        "    session    : {}\n",
        live.checkpoint.session.line()
    ));
    out.push_str(&format!(
        "    session by : {}\n",
        live.checkpoint.session.source_line()
    ));
    out.push_str(&format!(
        "    session read: {} ({}) -- {} session transcript(s)\n",
        live.checkpoint.session.read().source,
        live.checkpoint.session.read().age,
        live.checkpoint.session.read().candidates
    ));
    for note in live.checkpoint.session.notes() {
        out.push_str(&format!("    session    : {note}\n"));
    }
    out.push_str(&format!(
        "    command    : {}\n",
        live.checkpoint.command_preview
    ));
    out.push_str(&format!(
        "    ruling     : {}\n",
        match &live.checkpoint.refuse_reason {
            Some(reason) => format!("refused -- {reason}"),
            None => "press Enter in the TUI for the default canned ruling, or e for the chooser"
                .to_string(),
        }
    ));
}

fn render_inspect(out: &mut String, snapshot: &Snapshot) {
    let inspect = &snapshot.inspect;
    out.push_str("\n== INSPECT: the machinery ==\n");
    out.push_str(&format!(
        "  config : {} ({})\n",
        inspect.config_source, inspect.config_age
    ));
    out.push_str("\n  config seams (value, and whether it is still a Komun default):\n");
    for seam in &inspect.seams {
        out.push_str(&format!(
            "    {:<52} {:<36} {}\n",
            seam.key,
            seam.value,
            crate::ui::style::seam_word(seam.class)
        ));
    }
    out.push_str("\n  role x mount matrix (roles.mounts):\n");
    out.push_str("    role              workspace  memory     build-cache\n");
    for mount in &inspect.mounts {
        out.push_str(&format!(
            "    {:<17} {:<10} {:<10} {}\n",
            mount.role, mount.workspace, mount.memory, mount.build_cache
        ));
    }
    out.push_str(&format!(
        "\n  grant grid -- source {} ({})\n",
        inspect.grant_source, inspect.grant_age
    ));
    out.push_str(&format!(
        "    project {}   servers: {}\n",
        inspect.grant_project,
        inspect.grant_servers.join(", ")
    ));
    for row in &inspect.grants {
        let tools: Vec<String> = row
            .groups
            .iter()
            .map(|(server, names)| format!("{server}:{}", names.join(",")))
            .collect();
        out.push_str(&format!(
            "    {:<17} ceiling {:<9} {}\n",
            row.role,
            row.ceiling,
            if tools.is_empty() {
                "no grant".to_string()
            } else {
                tools.join("  ")
            }
        ));
    }
    out.push_str("\n  suites and gates (toolchain.commands + the gate journal):\n");
    for suite in &inspect.suites {
        out.push_str(&format!(
            "    {} {:<14} {}\n        file: {}\n        argv: {}\n",
            suite.light.glyph(),
            suite.gate,
            suite.last,
            suite.path,
            suite.argv
        ));
    }
    out.push_str(&format!(
        "\n  conversion candidates -- source {} ({})\n",
        inspect.conversion_source, inspect.conversion_age
    ));
    for row in &inspect.conversions {
        out.push_str(&format!(
            "    {:<52} {:<48} next review {}\n",
            row.step, row.status, row.next_review
        ));
        out.push_str(&format!("        {}\n", row.source));
    }
    out.push_str("\n  ADRs:\n");
    for adr in &inspect.adrs {
        out.push_str(&format!(
            "    {} -- {}\n        {}\n",
            adr.file, adr.title, adr.meta
        ));
    }
}
