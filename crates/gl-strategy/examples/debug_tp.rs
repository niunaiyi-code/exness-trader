//! 用于调试 tp_engine 的快速测试

use rust_decimal_macros::dec;

fn main() {
    let required_profit = dec!(10.0);
    let total_lots = dec!(0.01);
    let profit_per_dollar_per_lot = dec!(10000);
    let pip = dec!(0.1);
    let trailing_cushion_points = dec!(35.0);
    let trailing_step_points = dec!(25.0);

    let cushion_points = required_profit / (total_lots * profit_per_dollar_per_lot * pip);
    println!("cushion_points = {}", cushion_points);
    println!("cushion >= 35? {}", cushion_points >= trailing_cushion_points);

    let trigger_profit = required_profit + (trailing_step_points * total_lots * profit_per_dollar_per_lot * pip);
    println!("trigger_profit = {}", trigger_profit);

    let current_profit = dec!(260.0);
    println!("current >= trigger? {}", current_profit >= trigger_profit);
}
