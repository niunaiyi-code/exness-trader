export interface FormattedMt5Price {
  base: string;
  pips: string;
  pipette: string;
}

/** MT5 风格价格分段：整数 / 两位大字 / 第三位小字上标 */
export function formatMt5Price(price: number): FormattedMt5Price {
  if (!price || price <= 0 || isNaN(price)) {
    return { base: '--', pips: '--', pipette: '' };
  }
  // Exness XAUUSD 报价：2 位或 3 位小数
  const has3Decimals = Math.round(price * 1000) % 10 !== 0;
  if (has3Decimals) {
    const s = price.toFixed(3);
    const [intP, decP] = s.split('.');
    return { base: intP + '.', pips: decP.slice(0, 2), pipette: decP.slice(2, 3) };
  }
  const s = price.toFixed(2);
  const [intP, decP] = s.split('.');
  return { base: intP + '.', pips: decP, pipette: '' };
}
