mod parse;

use anyhow::{Context, Result};

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("missing path to file as first argument")?;
    let file = std::fs::read_to_string(path.as_str()).context("failed to read file")?;
    let parser = parse::Parser::new();
    let parsed = parser
        .parse(file.as_str())
        .context("failed to parse file")?;

    let mut count: u32 = 0;
    let mut correct: u32 = 0;
    let mut loss: f64 = 0.;
    for round in parsed.rounds {
        for turn in round.turns {
            count += 1;
            correct += (turn.player == turn.mortal) as u32;
            let player = &turn.actions[turn.player];
            let mortal = &turn.actions[turn.mortal];
            loss += (mortal.q - player.q).abs() as f64;
        }
    }
    let average_loss = loss / count as f64;
    let correct_ratio = correct as f32 / count as f32;
    println!("average loss:  {average_loss:.3}\ncorrect ratio: {correct_ratio:.3}");

    Ok(())
}

#[derive(Debug, Default)]
struct Parsed {
    rounds: Vec<Round>,
}

#[derive(Debug, Default)]
struct Round {
    turns: Vec<Turn>,
}

#[derive(Debug)]
struct Turn {
    player: usize,
    mortal: usize,
    actions: Vec<Action>,
}

#[derive(Debug)]
struct Action {
    q: f32,
    #[allow(dead_code)]
    pi: f32,
}
