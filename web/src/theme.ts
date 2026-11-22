/**
 * 暗色主题统一色板。与 App.tsx 中 ConfigProvider 的 token 保持一致，
 * 组件内需要内联色值时一律引用此处，禁止再硬编码十六进制字面量。
 */
export const palette = {
  /** 页面底色 */
  bgBase: '#141414',
  /** 卡片 / 表格容器底色 */
  bgContainer: '#1f1f1f',
  /** 弹层（Modal / Dropdown）底色 */
  bgElevated: '#1f1f1f',
  /** 主边框 */
  border: '#303030',
  /** 次级边框 */
  borderSecondary: '#262626',
  /** 品牌金 */
  gold: '#d4af37',
  /** 次级文字 */
  textSecondary: '#8c8c8c',
  /** 涨 / 盈利（中式红涨绿跌仅适用于股价配色；此处盈亏正值用绿为国际惯例，与现有界面一致） */
  profit: '#52c41a',
  /** 跌 / 亏损 */
  loss: '#ff4d4f',
} as const;
