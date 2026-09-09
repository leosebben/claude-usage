mod app;
mod data;
mod ui;
mod usage;

use app::App;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("claude-usage: uso de tokens e custo do Claude Code em ~/.claude\n");
        println!("Lê os transcritos em $CLAUDE_HOME/projects (padrão ~/.claude).");
        println!("Teclas: 1-4 período, t métrica, Tab painel, ↑↓ mover, Enter filtrar, Esc limpar, r recarregar, ? ajuda, q sair");
        return Ok(());
    }

    let home = usage::claude_home()?;
    let mut app = App::new(home);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    while !app.quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
                _ => {}
            }
        }
        app.poll_load();
        app.tick();
    }
    Ok(())
}
