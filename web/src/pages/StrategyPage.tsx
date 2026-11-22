import React, { useEffect, useState } from 'react';
import {
  Card,
  Table,
  Button,
  Space,
  Tag,
  Modal,
  Form,
  Input,
  InputNumber,
  Select,
  Switch,
  Popconfirm,
  message,
  Row,
  Col,
  Statistic,
} from 'antd';
import {
  PlusOutlined,
  EditOutlined,
  DeleteOutlined,
  UsergroupAddOutlined,
  ExperimentOutlined,
  CheckCircleOutlined,
  LockOutlined,
} from '@ant-design/icons';
import { api, apiErrorMessage } from '../lib/api';

export interface Strategy {
  id: string;
  name: string;
  description: string;
  initial_lot: string;
  grid_step: number;
  grid_step_add: number;
  grid_step_sec: number;
  lot_mult: string;
  tp_distance_usd: string;
  lock_trigger_usd: string;
  params_locked: boolean;
  created_at: number;
  updated_at: number;
  bound_accounts: string[];
}

interface AccountListItem {
  id: string;
  account_id: string;
  state: string;
  strategy_id?: string;
  strategy_name?: string;
}

const StrategyPage: React.FC = () => {
  const [strategies, setStrategies] = useState<Strategy[]>([]);
  const [accounts, setAccounts] = useState<AccountListItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [isEditModalOpen, setIsEditModalOpen] = useState(false);
  const [isBindModalOpen, setIsBindModalOpen] = useState(false);
  const [editingStrategy, setEditingStrategy] = useState<Strategy | null>(null);
  const [bindingStrategy, setBindingStrategy] = useState<Strategy | null>(null);
  const [selectedAccountIds, setSelectedAccountIds] = useState<string[]>([]);
  const [bindParamsLocked, setBindParamsLocked] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [form] = Form.useForm();

  useEffect(() => {
    fetchData();
  }, []);

  const fetchData = async () => {
    setLoading(true);
    try {
      const [stratRes, accRes] = await Promise.all([
        api.get('/strategies'),
        api.get('/accounts'),
      ]);
      setStrategies(stratRes.data);
      setAccounts(accRes.data);
    } catch (error) {
      message.error(apiErrorMessage(error, '加载策略或账户数据失败'));
    } finally {
      setLoading(false);
    }
  };

  const handleOpenCreate = () => {
    setEditingStrategy(null);
    form.resetFields();
    form.setFieldsValue({
      initial_lot: '0.01',
      grid_step: 300,
      grid_step_add: 75,
      grid_step_sec: 15,
      lot_mult: '1.45',
      tp_distance_usd: '1.00',
      lock_trigger_usd: '1000.00',
      params_locked: false,
    });
    setIsEditModalOpen(true);
  };

  const handleOpenEdit = (strat: Strategy) => {
    setEditingStrategy(strat);
    form.resetFields();
    form.setFieldsValue({
      name: strat.name,
      description: strat.description,
      initial_lot: strat.initial_lot,
      grid_step: strat.grid_step,
      grid_step_add: strat.grid_step_add,
      grid_step_sec: strat.grid_step_sec,
      lot_mult: strat.lot_mult,
      tp_distance_usd: strat.tp_distance_usd,
      lock_trigger_usd: strat.lock_trigger_usd,
      params_locked: strat.params_locked,
    });
    setIsEditModalOpen(true);
  };

  const handleSaveStrategy = async () => {
    try {
      const values = await form.validateFields();
      setSubmitting(true);

      const payload = {
        name: values.name,
        description: values.description || '',
        initial_lot: String(values.initial_lot),
        grid_step: Number(values.grid_step),
        grid_step_add: Number(values.grid_step_add),
        grid_step_sec: Number(values.grid_step_sec),
        lot_mult: String(values.lot_mult),
        tp_distance_usd: String(values.tp_distance_usd),
        lock_trigger_usd: String(values.lock_trigger_usd),
        params_locked: Boolean(values.params_locked),
      };

      if (editingStrategy) {
        await api.put(`/strategies/${editingStrategy.id}`, payload);
        message.success('策略更新成功，已同步至关联交易员账号');
      } else {
        await api.post('/strategies', payload);
        message.success('新策略定义成功');
      }

      setIsEditModalOpen(false);
      fetchData();
    } catch (error) {
      message.error(apiErrorMessage(error, '保存策略失败'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteStrategy = async (id: string) => {
    try {
      await api.delete(`/strategies/${id}`);
      message.success('策略已成功删除');
      fetchData();
    } catch (error) {
      message.error(apiErrorMessage(error, '删除策略失败'));
    }
  };

  const handleOpenBind = (strat: Strategy) => {
    setBindingStrategy(strat);
    setSelectedAccountIds(strat.bound_accounts || []);
    setBindParamsLocked(true);
    setIsBindModalOpen(true);
  };

  const handleSaveBind = async () => {
    if (!bindingStrategy) return;
    setSubmitting(true);
    try {
      await api.post(`/strategies/${bindingStrategy.id}/bind`, {
        account_ids: selectedAccountIds,
        params_locked: bindParamsLocked,
      });
      message.success(`成功将策略【${bindingStrategy.name}】关联到选定账号`);
      setIsBindModalOpen(false);
      fetchData();
    } catch (error) {
      message.error(apiErrorMessage(error, '关联策略失败'));
    } finally {
      setSubmitting(false);
    }
  };

  const columns = [
    {
      title: '策略名称',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: Strategy) => (
        <div>
          <div style={{ fontWeight: 'bold', fontSize: 14, color: '#d4af37' }}>
            <ExperimentOutlined style={{ marginRight: 6 }} />
            {name}
          </div>
          <div style={{ fontSize: 12, color: '#8c8c8c', marginTop: 2 }}>
            {record.description || '无详细描述'}
          </div>
        </div>
      ),
    },
    {
      title: '网格与入场参数',
      key: 'params',
      render: (_: unknown, record: Strategy) => (
        <Space direction="vertical" size={2}>
          <div>
            <Tag color="blue">首单: {record.initial_lot} 手</Tag>
            <Tag color="geekblue">倍率: {record.lot_mult}x</Tag>
          </div>
          <div style={{ fontSize: 12, color: '#bfbfbf' }}>
            间距: <b>{record.grid_step}</b> 点 (递增 +{record.grid_step_add} 点) · 防抖: {record.grid_step_sec}s
          </div>
        </Space>
      ),
    },
    {
      title: '止盈与锁仓',
      key: 'risk',
      render: (_: unknown, record: Strategy) => (
        <Space direction="vertical" size={2}>
          <div>
            <Tag color="green">止盈: ${record.tp_distance_usd}</Tag>
            <Tag color="volcano">锁仓: ${record.lock_trigger_usd}</Tag>
          </div>
          <div>
            {record.params_locked ? (
              <Tag icon={<LockOutlined />} color="red">参数已锁定</Tag>
            ) : (
              <Tag color="default">允许自由调整</Tag>
            )}
          </div>
        </Space>
      ),
    },
    {
      title: '已关联交易员账号',
      dataIndex: 'bound_accounts',
      key: 'bound_accounts',
      render: (bound: string[]) => (
        <div>
          {bound && bound.length > 0 ? (
            <Space wrap>
              {bound.map((accId) => (
                <Tag color="gold" key={accId}>
                  {accId}
                </Tag>
              ))}
            </Space>
          ) : (
            <span style={{ color: '#595959', fontSize: 12 }}>暂无关联账号</span>
          )}
        </div>
      ),
    },
    {
      title: '操作',
      key: 'action',
      render: (_: unknown, record: Strategy) => (
        <Space>
          <Button
            type="primary"
            size="small"
            icon={<UsergroupAddOutlined />}
            onClick={() => handleOpenBind(record)}
          >
            关联账号
          </Button>
          <Button
            size="small"
            icon={<EditOutlined />}
            onClick={() => handleOpenEdit(record)}
          >
            编辑
          </Button>
          <Popconfirm
            title={`确定删除策略【${record.name}】？`}
            description="删除后，已关联账号将被解除关联。"
            onConfirm={() => handleDeleteStrategy(record.id)}
            okText="删除"
            cancelText="取消"
          >
            <Button size="small" danger icon={<DeleteOutlined />}>
              删除
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Row gutter={16} style={{ marginBottom: 16 }}>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic
              title="已定义策略数"
              value={strategies.length}
              prefix={<ExperimentOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic
              title="已关联策略账号"
              value={accounts.filter((a) => a.strategy_id).length}
              prefix={<CheckCircleOutlined />}
              valueStyle={{ color: '#52c41a' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={8}>
          <Card>
            <Statistic
              title="待分配策略账号"
              value={accounts.filter((a) => !a.strategy_id).length}
              valueStyle={{ color: accounts.filter((a) => !a.strategy_id).length > 0 ? '#faad14' : '#8c8c8c' }}
            />
          </Card>
        </Col>
      </Row>

      <Card
        title={
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span>策略配置库 (定义量化策略模板并下发到交易员账号)</span>
            <Button
              type="primary"
              icon={<PlusOutlined />}
              onClick={handleOpenCreate}
            >
              新建策略模板
            </Button>
          </div>
        }
      >
        <Table
          columns={columns}
          dataSource={strategies}
          rowKey="id"
          loading={loading}
          pagination={false}
        />
      </Card>

      {/* 新建/编辑策略 Modal */}
      <Modal
        title={editingStrategy ? `编辑策略: ${editingStrategy.name}` : '新建量化交易策略'}
        open={isEditModalOpen}
        onOk={handleSaveStrategy}
        onCancel={() => setIsEditModalOpen(false)}
        confirmLoading={submitting}
        width={680}
        okText="保存策略"
        cancelText="取消"
      >
        <Form form={form} layout="vertical" style={{ marginTop: 16 }}>
          <Row gutter={16}>
            <Col span={14}>
              <Form.Item
                label="策略名称"
                name="name"
                rules={[{ required: true, message: '请输入策略名称' }]}
              >
                <Input placeholder="例如：经典稳健型网格 / 突破高频型" />
              </Form.Item>
            </Col>
          </Row>

          <Form.Item label="策略描述与特征说明" name="description">
            <Input.TextArea
              rows={2}
              placeholder="说明该策略适用的行情形态、风控特征或收益目标..."
            />
          </Form.Item>

          <Row gutter={16}>
            <Col span={8}>
              <Form.Item
                label="初始手数"
                name="initial_lot"
                rules={[{ required: true }]}
                tooltip="首单入场手数，如标准账户 0.01"
              >
                <Input placeholder="0.01" />
              </Form.Item>
            </Col>
            <Col span={8}>
              <Form.Item
                label="网格基础步长 (Points)"
                name="grid_step"
                rules={[{ required: true }]}
                tooltip="基础加仓点数，黄金 300 点即 30.0 点数 ($3.00)"
              >
                <InputNumber min={50} max={5000} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
            <Col span={8}>
              <Form.Item
                label="加仓递增点数 (Points)"
                name="grid_step_add"
                rules={[{ required: true }]}
                tooltip="每递增一层网格额外增加的点数，如 75 点"
              >
                <InputNumber min={0} max={2000} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
          </Row>

          <Row gutter={16}>
            <Col span={8}>
              <Form.Item
                label="加仓倍率 (Multiplier)"
                name="lot_mult"
                rules={[{ required: true }]}
                tooltip="网格加仓手数乘数，推荐 1.35 ~ 1.50"
              >
                <Input placeholder="1.45" />
              </Form.Item>
            </Col>
            <Col span={8}>
              <Form.Item
                label="单组止盈目标 (USD)"
                name="tp_distance_usd"
                rules={[{ required: true }]}
                tooltip="整组网格的目标毛利润，达标触发平仓或放飞移动锁利"
              >
                <Input placeholder="1.00" />
              </Form.Item>
            </Col>
            <Col span={8}>
              <Form.Item
                label="浮亏锁仓阈值 (USD)"
                name="lock_trigger_usd"
                rules={[{ required: true }]}
                tooltip="单侧浮亏达到此金额时自动触发严格对冲锁仓"
              >
                <Input placeholder="1000.00" />
              </Form.Item>
            </Col>
          </Row>

          <Row gutter={16}>
            <Col span={24}>
              <Form.Item
                label="加仓防毛刺去抖冷却 (秒)"
                name="grid_step_sec"
                rules={[{ required: true }]}
                tooltip="防止单根急拉K线瞬间连续下多单的最小安全冷却间隔，默认 15s"
              >
                <InputNumber min={1} max={300} style={{ width: '100%' }} />
              </Form.Item>
            </Col>
          </Row>
        </Form>
      </Modal>

      {/* 关联交易员账号 Modal */}
      <Modal
        title={
          <div>
            <UsergroupAddOutlined style={{ marginRight: 8, color: '#1890ff' }} />
            关联交易员账号至【{bindingStrategy?.name}】
          </div>
        }
        open={isBindModalOpen}
        onOk={handleSaveBind}
        onCancel={() => setIsBindModalOpen(false)}
        confirmLoading={submitting}
        okText="确认关联并下发参数"
        cancelText="取消"
      >
        <div style={{ margin: '16px 0', color: '#8c8c8c', fontSize: 13, lineHeight: '1.6' }}>
          💡 选中的交易员账号将绑定此策略模板。当账号在驾驶舱启动运行时，<b>策略参数将一次性载入内存执行</b>，交易全程零磁盘/数据库 I/O 磨损。
        </div>
        <Form.Item label="选择要关联的交易员账户">
          <Select
            mode="multiple"
            style={{ width: '100%' }}
            placeholder="请选择交易员账号"
            value={selectedAccountIds}
            onChange={setSelectedAccountIds}
            options={accounts.map((acc) => ({
              label: `${acc.account_id} (${acc.state === 'running' ? '运行中' : '就绪'} · 当前策略: ${acc.strategy_name || '未关联'})`,
              value: acc.id,
            }))}
          />
        </Form.Item>
        <Form.Item
          label="防篡改参数锁定"
          tooltip="开启后，被关联的交易员账号将禁止私自修改下发的策略参数；关闭则允许交易员在驾驶舱内自行微调"
        >
          <Switch
            checked={bindParamsLocked}
            onChange={setBindParamsLocked}
            checkedChildren="锁定"
            unCheckedChildren="开放"
          />
        </Form.Item>
      </Modal>
    </div>
  );
};

export default StrategyPage;
