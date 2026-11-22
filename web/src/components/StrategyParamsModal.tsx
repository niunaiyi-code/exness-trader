import React, { useState, useEffect } from 'react';
import { Modal, Form, Row, Col, Input, InputNumber, Button, Tag, message } from 'antd';
import {
  SettingOutlined,
  UnlockOutlined,
  SaveOutlined,
  AppstoreOutlined,
  SafetyCertificateOutlined,
} from '@ant-design/icons';
import { api, apiErrorMessage } from '../lib/api';
import { palette } from '../theme';

export interface StrategyParamsData {
  account_id: string;
  initial_lot: string;
  grid_step: number;
  grid_step_add: number;
  grid_step_sec: number;
  lot_mult: string;
  tp_distance_usd: string;
  lock_trigger_usd: string;
  params_locked: boolean;
}

export interface StrategyParamsModalProps {
  open: boolean;
  onClose: () => void;
  accountId: string;
  params?: StrategyParamsData | null;
  onParamsSaved?: (newParams: StrategyParamsData) => void;
}

export const StrategyParamsModal: React.FC<StrategyParamsModalProps> = ({
  open,
  onClose,
  accountId,
  params,
  onParamsSaved,
}) => {
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  // 当弹窗打开或外部 params 变更时同步表单数据
  useEffect(() => {
    if (open && params) {
      form.setFieldsValue({
        initial_lot: params.initial_lot,
        grid_step: params.grid_step,
        grid_step_add: params.grid_step_add,
        grid_step_sec: params.grid_step_sec,
        lot_mult: params.lot_mult,
        tp_distance_usd: params.tp_distance_usd,
        lock_trigger_usd: params.lock_trigger_usd,
      });
    }
  }, [open, params, form]);

  const handleFinish = async (values: Record<string, unknown>) => {
    if (!accountId) return;
    setSaving(true);
    try {
      const resp = await api.put<StrategyParamsData>(`/accounts/${accountId}/params`, {
        initial_lot: String(values.initial_lot ?? '0.01'),
        grid_step: Number(values.grid_step ?? 300),
        grid_step_add: Number(values.grid_step_add ?? 75),
        grid_step_sec: Number(values.grid_step_sec ?? 15),
        lot_mult: String(values.lot_mult ?? '1.45'),
        tp_distance_usd: String(values.tp_distance_usd ?? '1.00'),
        lock_trigger_usd: String(values.lock_trigger_usd ?? '1000.00'),
      });
      message.success('策略运行参数已成功保存并实时热应用至实盘引擎！');
      onParamsSaved?.(resp.data);
      onClose();
    } catch (error) {
      message.error(apiErrorMessage(error, '参数更新失败'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title={
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingRight: 32 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <SettingOutlined style={{ color: palette.gold, fontSize: 20 }} />
            <span style={{ color: '#ffffff', fontWeight: 600, fontSize: 16 }}>
              实盘策略运行参数配置
            </span>
            <Tag color="cyan" style={{ fontSize: 12, margin: 0, padding: '0 6px' }}>
              XAUUSD 黄金策略
            </Tag>
          </div>
          <Tag color="success" icon={<UnlockOutlined />} style={{ fontSize: 12, margin: 0 }}>
            自主微调模式已开放
          </Tag>
        </div>
      }
      open={open}
      onCancel={onClose}
      footer={null}
      width={880}
      destroyOnClose
      styles={{
        header: {
          paddingBottom: 14,
          borderBottom: `1px solid ${palette.borderSecondary}`,
          marginBottom: 16,
        },
        body: {
          padding: '0 4px',
        },
      }}
    >

      <Form
        form={form}
        layout="vertical"
        onFinish={handleFinish}
        initialValues={params || undefined}
        requiredMark={false}
      >
        {/* 分组一：网格建仓与进阶间距体系 */}
        <div
          style={{
            background: '#161616',
            border: `1px solid ${palette.borderSecondary}`,
            borderRadius: 8,
            padding: '16px 20px 8px',
            marginBottom: 18,
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              marginBottom: 16,
              borderBottom: '1px solid #252525',
              paddingBottom: 8,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <AppstoreOutlined style={{ color: palette.gold, fontSize: 15 }} />
              <span style={{ color: '#f0f0f0', fontWeight: 'bold', fontSize: 14 }}>
                网格建仓与加仓进阶体系
              </span>
            </div>
            <span style={{ color: palette.textSecondary, fontSize: 12 }}>
              控制首单仓位大小、逆势补仓间距与去抖机制
            </span>
          </div>

          <Row gutter={[20, 16]}>
            <Col xs={24} sm={8}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>初始首单手数</span>}
                name="initial_lot"
                rules={[{ required: true, message: '请输入初始手数' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    每轮首个方向开单的默认建仓手数
                  </span>
                }
              >
                <Input
                  placeholder="0.01"
                  suffix={<span style={{ color: palette.textSecondary, fontSize: 12 }}>手</span>}
                />
              </Form.Item>
            </Col>

            <Col xs={24} sm={8}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>加仓倍增系数</span>}
                name="lot_mult"
                rules={[{ required: true, message: '请输入倍增系数' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    每增加一层网格时仓位倍增比率
                  </span>
                }
              >
                <Input
                  placeholder="1.45"
                  suffix={<span style={{ color: palette.textSecondary, fontSize: 12 }}>倍</span>}
                />
              </Form.Item>
            </Col>

            <Col xs={24} sm={8}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>去抖冷却间隔</span>}
                name="grid_step_sec"
                rules={[{ required: true, message: '请输入去抖秒数' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    急速行情补仓的最小安全防抖时间
                  </span>
                }
              >
                <InputNumber
                  min={1}
                  max={3600}
                  style={{ width: '100%' }}
                  addonAfter="秒"
                  placeholder="15"
                />
              </Form.Item>
            </Col>
          </Row>

          <Row gutter={[20, 16]}>
            <Col xs={24} sm={12}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>基础加仓点距</span>}
                name="grid_step"
                rules={[{ required: true, message: '请输入加仓点距' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    初始逆势补仓基础点数间距（如 300 点 = 30 pips）
                  </span>
                }
              >
                <InputNumber
                  min={50}
                  max={5000}
                  style={{ width: '100%' }}
                  addonAfter="点"
                  placeholder="300"
                />
              </Form.Item>
            </Col>

            <Col xs={24} sm={12}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>间距阶梯递增</span>}
                name="grid_step_add"
                rules={[{ required: true, message: '请输入间距递增' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    每深入一层网格时，加仓间距额外递增的点数（如 75 点）
                  </span>
                }
              >
                <InputNumber
                  min={0}
                  max={2000}
                  style={{ width: '100%' }}
                  addonAfter="点"
                  placeholder="75"
                />
              </Form.Item>
            </Col>
          </Row>
        </div>

        {/* 分组二：止盈目标与极端锁仓风控 */}
        <div
          style={{
            background: '#161616',
            border: `1px solid ${palette.borderSecondary}`,
            borderRadius: 8,
            padding: '16px 20px 8px',
            marginBottom: 20,
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              marginBottom: 16,
              borderBottom: '1px solid #252525',
              paddingBottom: 8,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <SafetyCertificateOutlined style={{ color: '#52c41a', fontSize: 15 }} />
              <span style={{ color: '#f0f0f0', fontWeight: 'bold', fontSize: 14 }}>
                综合止盈收割与对冲风控
              </span>
            </div>
            <span style={{ color: palette.textSecondary, fontSize: 12 }}>
              控制持仓均价脱离成本区的止盈离场距离及全锁仓底线
            </span>
          </div>

          <Row gutter={[20, 16]}>
            <Col xs={24} sm={12}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>单层止盈距离</span>}
                name="tp_distance_usd"
                rules={[{ required: true, message: '请输入止盈距离' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    综合持仓均价脱离成本区达到此美元距离时触发整体止盈离场
                  </span>
                }
              >
                <Input
                  placeholder="1.00"
                  prefix={<span style={{ color: palette.textSecondary, marginRight: 2 }}>$</span>}
                  suffix={<span style={{ color: palette.textSecondary, fontSize: 12 }}>USD</span>}
                />
              </Form.Item>
            </Col>

            <Col xs={24} sm={12}>
              <Form.Item
                label={<span style={{ fontWeight: 600 }}>锁仓阈值</span>}
                name="lock_trigger_usd"
                rules={[{ required: true, message: '请输入锁仓阈值' }]}
                extra={
                  <span style={{ color: palette.textSecondary, fontSize: 12 }}>
                    浮亏达到此金额时自动触发严格对冲全锁仓保护本金
                  </span>
                }
              >
                <Input
                  placeholder="1000.00"
                  prefix={<span style={{ color: palette.textSecondary, marginRight: 2 }}>$</span>}
                  suffix={<span style={{ color: palette.textSecondary, fontSize: 12 }}>USD</span>}
                />
              </Form.Item>
            </Col>
          </Row>
        </div>

        {/* 底部操作区 */}
        <div
          style={{
            display: 'flex',
            justifyContent: 'flex-end',
            alignItems: 'center',
            gap: 12,
            paddingTop: 12,
            borderTop: `1px solid ${palette.borderSecondary}`,
          }}
        >
          <Button onClick={onClose} style={{ minWidth: 80 }}>
            取消
          </Button>
          <Button
            type="primary"
            htmlType="submit"
            icon={<SaveOutlined />}
            loading={saving}
            style={{
              minWidth: 160,
              fontWeight: 500,
            }}
          >
            保存并热应用到实盘
          </Button>
        </div>
      </Form>
    </Modal>
  );
};

export default StrategyParamsModal;
