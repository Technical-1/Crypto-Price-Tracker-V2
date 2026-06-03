use std::io::{self, Stdout};
use std::panic;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crypto_price_tracker_v2::app::{App, View};
use crypto_price_tracker_v2::config::Config;
use crypto_price_tracker_v2::event::{apply, map_key, Action};
use crypto_price_tracker_v2::ledger::{self, load_ledger};
use crypto_price_tracker_v2::perf;
use crypto_price_tracker_v2::prices::cache::PriceCache;
use crypto_price_tracker_v2::prices::coingecko::CoinGeckoSource;
use crypto_price_tracker_v2::prices::{PriceBook, PriceSource};
use crypto_price_tracker_v2::{export, ui};

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Parser, Debug)]
#[command(name = "crypto-price-tracker-v2")]
struct Args {
    #[arg(long, default_value = "config.json")]
    config: String,
    #[arg(long)]
    ledger: Option<String>,
    #[arg(long)]
    offline: bool,
}

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        // Best-effort restore using a fresh stdout handle (the live Terminal is
        // not reachable from here during unwind).
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config).context("loading config")?;
    let ledger_path = args
        .ledger
        .clone()
        .unwrap_or_else(|| config.ledger_path.clone());
    let txs = load_ledger(&ledger_path).context("loading ledger")?;
    let asset_ids: Vec<String> = ledger::assets(&txs).into_iter().collect();

    let mut app = App::new(config.clone(), &txs).context("building app")?;
    let cache = PriceCache::new(config.cache.expanded_dir(), config.cache.ttl_seconds);
    let vs = config.display_currency.clone();
    let history_path = "history.json".to_string();

    if let Ok(Some(book)) = cache.load_fresh() {
        app.set_prices(book);
    } else if let Ok(Some(book)) = cache.load_last_good() {
        app.set_prices(book);
    }

    // Load any existing history for the Performance view.
    if let Ok(h) = perf::load_history(&history_path) {
        app.history = h;
    }

    install_panic_hook();
    let mut terminal = setup_terminal()?;

    let (tx, mut rx) = mpsc::channel::<Result<PriceBook, String>>(4);
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));

    let spawn_fetch = |tx: mpsc::Sender<Result<PriceBook, String>>| {
        let ids = asset_ids.clone();
        let vs = vs.clone();
        tokio::spawn(async move {
            let source = CoinGeckoSource::new();
            let msg = source.fetch(&ids, &vs).await.map_err(|e| e.to_string());
            let _ = tx.send(msg).await;
        });
    };

    if !args.offline {
        app.loading = true;
        spawn_fetch(tx.clone());
    }

    let res = run(
        &mut app,
        &mut terminal,
        &mut events,
        &mut rx,
        &tx,
        &cache,
        &history_path,
        config.refresh_seconds,
        args.offline,
        &mut tick,
        spawn_fetch,
    )
    .await;

    restore_terminal(&mut terminal)?;
    res
}

#[allow(clippy::too_many_arguments)]
async fn run(
    app: &mut App,
    terminal: &mut Tui,
    events: &mut EventStream,
    rx: &mut mpsc::Receiver<Result<PriceBook, String>>,
    tx: &mpsc::Sender<Result<PriceBook, String>>,
    cache: &PriceCache,
    history_path: &str,
    refresh_seconds: u64,
    offline: bool,
    tick: &mut tokio::time::Interval,
    spawn_fetch: impl Fn(mpsc::Sender<Result<PriceBook, String>>),
) -> Result<()> {
    // Countdown (in seconds) to the next automatic refresh; the 1s tick drives it.
    let mut secs_left = refresh_seconds.max(1);
    app.seconds_to_refresh = secs_left;
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        if app.should_quit {
            return Ok(());
        }

        tokio::select! {
            maybe_event = events.next() => {
                if let Some(Ok(Event::Key(key))) = maybe_event {
                    if key.kind == KeyEventKind::Press {
                        if let Some(action) = map_key(key) {
                            let wants_refresh = apply(app, action);
                            if action == Action::Export {
                                do_export(app);
                            }
                            if wants_refresh && !offline {
                                spawn_fetch(tx.clone());
                            }
                        }
                    }
                }
            }
            _ = tick.tick() => {
                if !offline {
                    if secs_left == 0 {
                        app.loading = true;
                        spawn_fetch(tx.clone());
                        secs_left = refresh_seconds.max(1);
                    } else {
                        secs_left -= 1;
                    }
                    app.seconds_to_refresh = secs_left;
                }
            }
            Some(result) = rx.recv() => {
                match result {
                    Ok(book) => {
                        let _ = cache.store(&book);
                        app.set_prices(book);
                        if let Some(report) = &app.derived.valuation {
                            let _ = perf::record_snapshot(
                                history_path, report.total_value, Utc::now(), refresh_seconds as i64,
                            );
                        }
                        if let Ok(h) = perf::load_history(history_path) {
                            app.history = h;
                        }
                    }
                    Err(e) => {
                        app.loading = false;
                        app.status.message = format!("fetch error: {e}");
                    }
                }
            }
        }
    }
}

fn do_export(app: &mut App) {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    match app.view {
        View::Tax => {
            if let Some(cg) = &app.derived.capital_gains {
                let csv = format!("capital-gains-{}-{}.csv", app.tax_year, stamp);
                let json = format!("capital-gains-{}-{}.json", app.tax_year, stamp);
                let result = export::export_capital_gains_csv(cg, &csv)
                    .and_then(|()| export::export_capital_gains_json(cg, &json));
                app.status.message = match result {
                    Ok(()) => format!("exported {csv} + {json}"),
                    Err(e) => format!("export failed: {e}"),
                };
            }
        }
        View::Holdings => {
            let holdings: Vec<_> = app
                .derived
                .holdings
                .iter()
                .map(|h| h.holding.clone())
                .collect();
            let csv = format!("holdings-{}.csv", stamp);
            let json = format!("holdings-{}.json", stamp);
            let result = export::export_holdings_csv(&holdings, &csv)
                .and_then(|()| export::export_holdings_json(&holdings, &json));
            app.status.message = match result {
                Ok(()) => format!("exported {csv} + {json}"),
                Err(e) => format!("export failed: {e}"),
            };
        }
        _ => app.status.message = "export available on Tax/Holdings views".into(),
    }
}
