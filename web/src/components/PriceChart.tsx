import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  createChart,
  ColorType,
  LineStyle,
  CandlestickSeries,
  IChartApi,
  ISeriesApi,
  IPriceLine,
  UTCTimestamp,
  CandlestickData,
} from 'lightweight-charts';
import { Segmented } from 'antd';
import { api } from '../lib/api';

export type Timeframe = 'M1' | 'M5' | 'M15' | 'M30' | 'H1' | 'H4' | 'D1' | 'W1' | 'MN';

const TIMEFRAME_SECONDS: Record<Timeframe, number> = {
  M1: 60,
  M5: 5 * 60,
  M15: 15 * 60,
  M30: 30 * 60,
  H1: 3600,
  H4: 4 * 3600,
  D1: 86400,
  W1: 7 * 86400,
  MN: 30 * 86400,
};

interface Candle {
  time: UTCTimestamp;
  open: number;
  high: number;
  low: number;
  close: number;
}

async function fetchCandles(tf: Timeframe): Promise<CandlestickData<UTCTimestamp>[]> {
  try {
    const resp = await api.get<Candle[]>('/market/candles', {
      params: { symbol: 'XAUUSD', timeframe: tf, count: 300 },
    });
    return resp.data || [];
  } catch (err) {
    console.warn(`[PriceChart] Failed to fetch candles for timeframe ${tf}:`, err);
    return [];
  }
}

interface PriceChartProps {
  currentBid: number;
  currentAsk: number;
  buyTpPrice?: number | null;
  sellTpPrice?: number | null;
  height?: number;
}

export const PriceChart: React.FC<PriceChartProps> = ({
  currentBid,
  currentAsk,
  buyTpPrice,
  sellTpPrice,
  height = 360,
}) => {
  const [timeframe, setTimeframe] = useState<Timeframe>('M1');
  const [chartReady, setChartReady] = useState(false);
  // 只跟踪止盈线的纵坐标（bid/ask 由图表原生价格线呈现，无需浮层换算）
  const [tpCoords, setTpCoords] = useState<{ buyTpY: number | null; sellTpY: number | null }>({
    buyTpY: null,
    sellTpY: null,
  });

  const chartContainerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const candleSeriesRef = useRef<ISeriesApi<'Candlestick'> | null>(null);
  const buyTpLineRef = useRef<IPriceLine | null>(null);
  const sellTpLineRef = useRef<IPriceLine | null>(null);
  const askLineRef = useRef<IPriceLine | null>(null);
  const buyTpPriceRef = useRef<number | null>(buyTpPrice || null);
  const sellTpPriceRef = useRef<number | null>(sellTpPrice || null);
  const lastBarRef = useRef<Candle | null>(null);

  // 从 ref 读取最新止盈价，保证回调引用稳定（可安全进入任意 effect 依赖）
  const updateTpCoords = useCallback(() => {
    requestAnimationFrame(() => {
      const series = candleSeriesRef.current;
      if (!series) return;
      const buyTp = buyTpPriceRef.current;
      const sellTp = sellTpPriceRef.current;
      try {
        setTpCoords({
          buyTpY: buyTp && buyTp > 0 ? series.priceToCoordinate(buyTp) : null,
          sellTpY: sellTp && sellTp > 0 ? series.priceToCoordinate(sellTp) : null,
        });
      } catch {
        // 图表销毁瞬间可能抛错，忽略
      }
    });
  }, []);

  useEffect(() => {
    buyTpPriceRef.current = buyTpPrice || null;
    sellTpPriceRef.current = sellTpPrice || null;
    updateTpCoords();
  }, [buyTpPrice, sellTpPrice, updateTpCoords]);

  useEffect(() => {
    if (!chartContainerRef.current) return;

    const chart = createChart(chartContainerRef.current, {
      layout: {
        background: { type: ColorType.Solid, color: '#000000' }, // MT5 经典纯黑背景
        textColor: '#d9d9d9',
      },
      grid: {
        vertLines: { color: '#1a1a1a', style: LineStyle.Dotted },
        horzLines: { color: '#1a1a1a', style: LineStyle.Dotted },
      },
      crosshair: {
        vertLine: { color: '#595959', style: LineStyle.Dashed },
        horzLine: { color: '#595959', style: LineStyle.Dashed },
      },
      timeScale: {
        borderColor: '#262626',
        timeVisible: true,
        secondsVisible: false,
        rightOffset: 12, // MT5 经典图表平移：右侧留出 12 根 K 线缓冲
      },
      rightPriceScale: {
        borderColor: '#262626',
        autoScale: true,
      },
      height,
    });

    // MT5 官方标准 "Green on Black" 预设烛形配色：
    // 阳烛：纯白实心 + 亮绿边框/影线；阴烛：纯黑镂空 + 亮绿边框/影线
    const candleSeries = chart.addSeries(CandlestickSeries, {
        upColor: '#ffffff',
        downColor: '#000000',
        borderVisible: true,
        borderUpColor: '#00ff00',
        borderDownColor: '#00ff00',
        wickVisible: true,
        wickUpColor: '#00ff00',
        wickDownColor: '#00ff00',
        priceLineVisible: true,
        priceLineColor: '#8c8c8c', // MT5 买价线 (Bid 灰色细点虚线)
        priceLineStyle: LineStyle.Dotted,
        priceLineWidth: 1,
        title: '',
        autoscaleInfoProvider: (original: () => {
          priceRange?: { minValue: number; maxValue: number };
        } | null) => {
          const res = original();
          if (!res || !res.priceRange) return res;
          let min = res.priceRange.minValue;
          let max = res.priceRange.maxValue;
          if (buyTpPriceRef.current && buyTpPriceRef.current > 0) {
            min = Math.min(min, buyTpPriceRef.current - 0.5);
            max = Math.max(max, buyTpPriceRef.current + 0.5);
          }
          if (sellTpPriceRef.current && sellTpPriceRef.current > 0) {
            min = Math.min(min, sellTpPriceRef.current - 0.5);
            max = Math.max(max, sellTpPriceRef.current + 0.5);
          }
          return { ...res, priceRange: { minValue: min, maxValue: max } };
        },
      },
    );

    chartRef.current = chart;
    candleSeriesRef.current = candleSeries;
    setChartReady(true);

    chart.timeScale().subscribeVisibleLogicalRangeChange(() => {
      updateTpCoords();
    });
    chart.subscribeCrosshairMove(() => {
      updateTpCoords();
    });
    setTimeout(updateTpCoords, 100);

    const handleResize = () => {
      if (chartContainerRef.current && chartRef.current) {
        const width = chartContainerRef.current.clientWidth;
        if (width > 0) {
          chartRef.current.applyOptions({
            width,
          });
          updateTpCoords();
        }
      }
    };

    // 重点：使用 ResizeObserver 精准监听挂载容器尺寸变动（右侧折叠、Grid 变换等，不依赖全局 window.resize）
    const resizeObserver = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width } = entry.contentRect;
        if (width > 0 && chartRef.current) {
          chartRef.current.applyOptions({ width: Math.floor(width) });
          updateTpCoords();
        }
      }
    });

    if (chartContainerRef.current) {
      resizeObserver.observe(chartContainerRef.current);
    }

    window.addEventListener('resize', handleResize);
    handleResize();

    return () => {
      resizeObserver.disconnect();
      window.removeEventListener('resize', handleResize);
      buyTpLineRef.current = null;
      sellTpLineRef.current = null;
      askLineRef.current = null;
      chart.remove();
      setChartReady(false);
    };
  }, [updateTpCoords]);

  // 高度自适应：当外部传入的 height 改变时平滑更新尺寸，绝不销毁和重建图表
  useEffect(() => {
    if (chartRef.current && height > 0) {
      chartRef.current.applyOptions({ height });
      updateTpCoords();
    }
  }, [height, updateTpCoords]);

  // 格式化实时价差
  const formatGap = (diff: number) => {
    const sign = diff >= 0 ? '+' : '';
    return `${sign}${diff.toFixed(2)}`;
  };

  // 加载当前周期的真实历史 K 线
  useEffect(() => {
    const series = candleSeriesRef.current;
    if (!series || !chartReady) return;

    let cancelled = false;
    fetchCandles(timeframe).then((data) => {
      if (cancelled || !data.length) return;
      series.setData(data);
      lastBarRef.current = data[data.length - 1];
      if (chartRef.current) {
        chartRef.current.timeScale().fitContent();
      }
      updateTpCoords();
    });

    return () => {
      cancelled = true;
    };
  }, [timeframe, chartReady, updateTpCoords]);

  // 实时 bid tick 时更新当前 K 线
  useEffect(() => {
    const series = candleSeriesRef.current;
    if (!series || !currentBid || currentBid <= 0) return;

    const intervalSec = TIMEFRAME_SECONDS[timeframe];
    const nowSec = Math.floor(Date.now() / 1000);
    const currentBucket = Math.floor(nowSec / intervalSec) * intervalSec;

    if (!lastBarRef.current || currentBucket > lastBarRef.current.time) {
      lastBarRef.current = {
        time: currentBucket as UTCTimestamp,
        open: currentBid,
        high: currentBid,
        low: currentBid,
        close: currentBid,
      };
    } else if (currentBucket === lastBarRef.current.time) {
      lastBarRef.current.high = Math.max(lastBarRef.current.high, currentBid);
      lastBarRef.current.low = Math.min(lastBarRef.current.low, currentBid);
      lastBarRef.current.close = currentBid;
    }

    try {
      series.update(lastBarRef.current);
    } catch {
      // 时间戳回退时图表库可能抛错，忽略
    }
  }, [currentBid, timeframe]);

  // 止盈价格线与卖价线（图表原生能力，不经过 React 渲染）
  useEffect(() => {
    const series = candleSeriesRef.current;
    if (!series) return;

    // 多头止盈线 - 翠绿色实线
    if (buyTpPrice && buyTpPrice > 0) {
      if (!buyTpLineRef.current) {
        buyTpLineRef.current = series.createPriceLine({
          price: buyTpPrice,
          color: '#00e676',
          lineWidth: 2,
          lineStyle: LineStyle.Solid,
          axisLabelVisible: true,
          title: '',
        });
      } else {
        buyTpLineRef.current.applyOptions({ price: buyTpPrice, title: '' });
      }
    } else if (buyTpLineRef.current) {
      try {
        series.removePriceLine(buyTpLineRef.current);
      } catch {
        // 忽略
      }
      buyTpLineRef.current = null;
    }

    // 空头止盈线 - 亮绿色实线
    if (sellTpPrice && sellTpPrice > 0) {
      if (!sellTpLineRef.current) {
        sellTpLineRef.current = series.createPriceLine({
          price: sellTpPrice,
          color: '#00e676',
          lineWidth: 2,
          lineStyle: LineStyle.Solid,
          axisLabelVisible: true,
          title: '',
        });
      } else {
        sellTpLineRef.current.applyOptions({ price: sellTpPrice, title: '' });
      }
    } else if (sellTpLineRef.current) {
      try {
        series.removePriceLine(sellTpLineRef.current);
      } catch {
        // 忽略
      }
      sellTpLineRef.current = null;
    }

    // MT5 经典卖价线 (Ask) - 红色细点虚线
    if (currentAsk && currentAsk > 0) {
      if (!askLineRef.current) {
        askLineRef.current = series.createPriceLine({
          price: currentAsk,
          color: '#ff4d4f',
          lineWidth: 1,
          lineStyle: LineStyle.Dotted,
          axisLabelVisible: true,
          title: '',
        });
      } else {
        askLineRef.current.applyOptions({ price: currentAsk, title: '' });
      }
    } else if (askLineRef.current) {
      try {
        series.removePriceLine(askLineRef.current);
      } catch {
        // 忽略
      }
      askLineRef.current = null;
    }
  }, [buyTpPrice, sellTpPrice, currentAsk, chartReady]);

  return (
    <div
      style={{
        position: 'relative',
        width: '100%',
        background: '#0a0a0a',
        borderRadius: 8,
        padding: '12px',
        border: '1px solid #1f1f1f',
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: 12,
          marginBottom: 10,
          fontSize: 13,
          color: '#8c8c8c',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <span style={{ color: '#d4af37', fontWeight: 'bold', fontSize: 14 }}>
            XAUUSD [{timeframe}] 实盘走势
          </span>
          <Segmented
            options={['M1', 'M5', 'M15', 'M30', 'H1', 'H4', 'D1', 'W1', 'MN']}
            value={timeframe}
            onChange={(val) => setTimeframe(val as Timeframe)}
            size="small"
          />
          <span>
            Bid: <b style={{ color: '#ff5252' }}>{currentBid.toFixed(2)}</b>
          </span>
          <span>
            Ask: <b style={{ color: '#00e676' }}>{currentAsk.toFixed(2)}</b>
          </span>
          <span>
            点差:{' '}
            <b style={{ color: '#faad14' }}>{(Math.max(0, currentAsk - currentBid) * 10).toFixed(1)} 点</b>
          </span>
        </div>
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          {buyTpPrice && buyTpPrice > 0 && (
            <span style={{ color: '#00e676', fontWeight: 'bold' }}>
              ● 多头止盈: ${buyTpPrice.toFixed(2)}{' '}
              {currentBid > 0 && (
                <span style={{ fontSize: 12, fontWeight: 'normal', color: '#a0d911' }}>
                  (价差 {formatGap(buyTpPrice - currentBid)})
                </span>
              )}
            </span>
          )}
          {sellTpPrice && sellTpPrice > 0 && (
            <span style={{ color: '#00e676', fontWeight: 'bold' }}>
              ● 空头止盈: ${sellTpPrice.toFixed(2)}{' '}
              {(currentAsk > 0 || currentBid > 0) && (
                <span style={{ fontSize: 12, fontWeight: 'normal', color: '#a0d911' }}>
                  (价差 {formatGap(sellTpPrice - (currentAsk > 0 ? currentAsk : currentBid))})
                </span>
              )}
            </span>
          )}
        </div>
      </div>
      <div style={{ position: 'relative', width: '100%', height }}>
        <div ref={chartContainerRef} style={{ width: '100%', height: '100%' }} />

        {/* MT5 右上角经典水印 */}
        <div
          style={{
            position: 'absolute',
            top: 8,
            right: 70,
            color: '#404040',
            fontSize: 12,
            fontFamily: 'monospace',
            fontWeight: 'bold',
            letterSpacing: 1,
            pointerEvents: 'none',
            zIndex: 3,
            userSelect: 'none',
          }}
        >
          GoldLegendEA
        </div>

        {/* 左侧止盈线描述浮层（避开右侧最新 K 线区域） */}
        <div
          style={{
            position: 'absolute',
            top: 0,
            left: 10,
            height: '100%',
            pointerEvents: 'none',
            zIndex: 5,
          }}
        >
          {tpCoords.buyTpY !== null && tpCoords.buyTpY >= 0 && tpCoords.buyTpY <= height && (
            <div style={{ position: 'absolute', top: tpCoords.buyTpY - 10, left: 0 }}>
              <span
                style={{
                  background: 'rgba(0, 230, 118, 0.25)',
                  border: '1px solid #00e676',
                  color: '#00e676',
                  fontSize: 11,
                  fontWeight: 'bold',
                  padding: '1px 6px',
                  borderRadius: 3,
                  whiteSpace: 'nowrap',
                  backdropFilter: 'blur(3px)',
                }}
              >
                多头止盈 (价差 {formatGap(buyTpPrice! - currentBid)})
              </span>
            </div>
          )}

          {tpCoords.sellTpY !== null && tpCoords.sellTpY >= 0 && tpCoords.sellTpY <= height && (
            <div style={{ position: 'absolute', top: tpCoords.sellTpY - 10, left: 0 }}>
              <span
                style={{
                  background: 'rgba(0, 230, 118, 0.25)',
                  border: '1px solid #00e676',
                  color: '#00e676',
                  fontSize: 11,
                  fontWeight: 'bold',
                  padding: '1px 6px',
                  borderRadius: 3,
                  whiteSpace: 'nowrap',
                  backdropFilter: 'blur(3px)',
                }}
              >
                空头止盈 (价差 {formatGap(sellTpPrice! - (currentAsk > 0 ? currentAsk : currentBid))})
              </span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default PriceChart;
