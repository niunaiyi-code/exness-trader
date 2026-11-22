/** XAUUSD：0.01 手 = 1 盎司，1 盎司波动 $1 = $1 → 每手每美元波动收益 = 100 */
export const GOLD_PROFIT_PER_DOLLAR_PER_LOT = 100;

/**
 * WS 行情尚未到达、用 K 线收盘价兜底时的默认点差（USD 报价单位）。
 * 仅作为加载瞬间的占位，真实 ask 一律以 WS 推送为准。
 */
export const DEFAULT_XAUUSD_SPREAD = 0.24;

/**
 * Exness 行情探针账号段约定：USD 标准账户为 18x 段，USC 美分账户为 25x 段。
 * 用于把 WS tick 按币种过滤（USD 账户只吃 USD 探针的价）。
 */
export const USD_PROBE_PREFIX = '18';
export const USC_PROBE_PREFIX = '25';
