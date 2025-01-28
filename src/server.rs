use warp::Filter;
use serde_json::json;
use crate::bot::ArbitrageBot;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let bot = Arc::new(Mutex::new(ArbitrageBot::new().unwrap()));
    let is_testing = Arc::new(Mutex::new(true)); // Default to testing mode

    let status_route = warp::path("status")
        .map({
            let bot = Arc::clone(&bot);
            move || {
                let bot = bot.lock().unwrap();
                let status = bot.get_status(); // Assuming this method exists
                warp::reply::json(&status)
            }
        });

    let logs_route = warp::path("logs")
        .map({
            let bot = Arc::clone(&bot);
            move || {
                // Replace with actual log retrieval logic
                let logs = vec!["Log 1", "Log 2", "Log 3"];
                warp::reply::json(&logs)
            }
        });

    let configure_route = warp::path("configure")
        .and(warp::post())
        .and(warp::body::json())
        .map({
            let bot = Arc::clone(&bot);
            let is_testing = Arc::clone(&is_testing);
            move |config: serde_json::Value| {
                let mut bot = bot.lock().unwrap();
                let mode = config["mode"].as_str().unwrap();
                let min_profit = config["min_profit"].as_f64().unwrap();
                let min_investment = config["min_investment"].as_f64().unwrap();

                // Update bot configuration
                bot.set_mode(mode); // Implement this method
                bot.set_min_profit(min_profit); // Implement this method
                bot.set_min_investment(min_investment); // Implement this method

                // Update testing mode
                let testing = mode == "testing";
                *is_testing.lock().unwrap() = testing;

                warp::reply::json(&json!({"status": "Configuration updated"}))
            }
        });

    let routes = status_route.or(logs_route).or(configure_route);

    println!("Server running on http://localhost:3030");
    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
} 