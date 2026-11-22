import React, { useEffect, useState } from 'react';
import { Card, Row, Col, Statistic, Table, Button, Space, Tag, Modal, Form, Input, Select, Popconfirm, message, Switch } from 'antd';
import { DollarOutlined, ShoppingOutlined, PlayCircleOutlined, PauseCircleOutlined, PlusOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { api, apiErrorMessage } from '../lib/api';

interface Account {
  id: string;
  account_id: string;
  state: string;
  strategy_id?: string;
  strategy_name?: string;
  permission?: string;
}

interface StrategyOption {
  id: string;
  name: string;
}

const DashboardPage: React.FC = () => {
  const navigate = useNavigate();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [strategies, setStrategies] = useState<StrategyOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [isAssignModalOpen, setIsAssignModalOpen] = useState(false);
  const [targetAccount, setTargetAccount] = useState<Account | null>(null);
  const [chosenStrategyId, setChosenStrategyId] = useState<string | undefined>(undefined);
  const [assignParamsLocked, setAssignParamsLocked] = useState(true);
  const [form] = Form.useForm();

  useEffect(() => {
    fetchAccounts();
  }, []);

  const fetchAccounts = async () => {
    setLoading(true);
    try {
      const [accRes, stratRes] = await Promise.allSettled([
        api.get('/accounts'),
        api.get('/strategies'),
      ]);
      if (accRes.status === 'fulfilled') {
        setAccounts(accRes.value.data);
      } else {
        console.warn('[dashboard] Failed to fetch accounts:', accRes.reason);
        message.error('获取账户列表失败');
      }
      if (stratRes.status === 'fulfilled') {
        setStrategies(stratRes.value.data);
      } else {
        console.warn('[dashboard] Failed to fetch strategies:', stratRes.reason);
      }
    } catch (err) {
      console.warn('[dashboard] fetchAccounts unexpected error:', err);
      message.error('获取数据失败');
    } finally {
      setLoading(false);
    }
  };

  const handleOpenAssign = (acc: Account) => {
    setTargetAccount(acc);
    setChosenStrategyId(acc.strategy_id);
    setAssignParamsLocked(true);
    setIsAssignModalOpen(true);
  };

  const handleSaveAssign = async () => {
    if (!targetAccount) return;
    setSubmitting(true);
    try {
      await api.post(`/accounts/${targetAccount.id}/strategy`, {
        strategy_id: chosenStrategyId || null,
        params_locked: assignParamsLocked,
      });
      message.success(`账户 ${targetAccount.account_id} 策略关联已更新`);
      setIsAssignModalOpen(false);
      fetchAccounts();
    } catch (error) {
      message.error(apiErrorMessage(error, '更新策略关联失败'));
    } finally {
      setSubmitting(false);
    }
  };

  const handleStart = async (id: string) => {
    try {
      await api.post(`/accounts/${id}/start`, {});
      message.success('已启动账户运行器');
      fetchAccounts();
    } catch (error) {
      message.error(apiErrorMessage(error, '启动失败'));
    }
  };

  const handleStop = async (id: string) => {
    try {
      await api.post(`/accounts/${id}/stop`, {});
      message.success('已停止账户运行器');
      fetchAccounts();
    } catch (error) {
      message.error(apiErrorMessage(error, '停止失败'));
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await api.delete(`/accounts/${id}`);
      message.success('账户已删除');
      fetchAccounts();
    } catch (error) {
      message.error(apiErrorMessage(error, '删除失败'));
    }
  };

  const handleCreateAccount = async (values: Record<string, unknown>) => {
    setSubmitting(true);
    try {
      await api.post('/accounts', values);
      message.success('账户接入成功');
      setIsModalOpen(false);
      form.resetFields();
      fetchAccounts();
    } catch (error) {
      message.error(apiErrorMessage(error, '创建账户失败'));
    } finally {
      setSubmitting(false);
    }
  };

  const columns = [
    {
      title: '账户 ID',
      dataIndex: 'account_id',
      key: 'account_id',
    },
    {
      title: '状态',
      dataIndex: 'state',
      key: 'state',
      render: (state: string) => {
        const colorMap: Record<string, string> = {
          running: 'green',
          init: 'blue',
          degraded: 'orange',
          failed: 'red',
        };
        return <Tag color={colorMap[state] || 'default'}>{state}</Tag>;
      },
    },
    {
      title: '关联策略',
      key: 'strategy',
      render: (_: unknown, record: Account) => (
        <Space>
          {record.strategy_name ? (
            <Tag color="gold">{record.strategy_name}</Tag>
          ) : (
            <Tag color="default">未关联</Tag>
          )}
          <Button
            type="link"
            size="small"
            onClick={() => handleOpenAssign(record)}
          >
            分配策略
          </Button>
        </Space>
      ),
    },
    {
      title: '权限',
      dataIndex: 'permission',
      key: 'permission',
      render: (perm: string) => (
        <Tag color={perm === 'admin' ? 'purple' : 'default'}>{perm || 'admin'}</Tag>
      ),
    },
    {
      title: '操作',
      key: 'action',
      render: (_: unknown, record: Account) => (
        <Space>
          {record.state === 'running' ? (
            <Button
              type="primary"
              danger
              size="small"
              icon={<PauseCircleOutlined />}
              onClick={() => handleStop(record.id)}
            >
              停止
            </Button>
          ) : (
            <Button
              type="primary"
              size="small"
              icon={<PlayCircleOutlined />}
              onClick={() => handleStart(record.id)}
            >
              启动
            </Button>
          )}
          <Button type="link" size="small" onClick={() => navigate(`/accounts/${record.id}`)}>
            详情
          </Button>
          <Popconfirm
            title="确认删除此账户？"
            onConfirm={() => handleDelete(record.id)}
            okText="确认"
            cancelText="取消"
          >
            <Button type="link" danger size="small">
              删除
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div>
      <Row gutter={16} style={{ marginBottom: 24 }}>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="活跃账户"
              value={accounts.filter((a) => a.state === 'running').length}
              prefix={<ShoppingOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="总账户数"
              value={accounts.length}
              prefix={<DollarOutlined />}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Statistic
              title="故障账户"
              value={accounts.filter((a) => a.state === 'failed').length}
              valueStyle={{ color: '#cf1322' }}
            />
          </Card>
        </Col>
        <Col xs={24} sm={12} lg={6}>
          <Card>
            <Button
              type="primary"
              icon={<PlusOutlined />}
              block
              onClick={() => setIsModalOpen(true)}
            >
              添加账户
            </Button>
          </Card>
        </Col>
      </Row>

      <Card title="账户列表" loading={loading}>
        <Table
          dataSource={accounts}
          columns={columns}
          rowKey="id"
          pagination={{ pageSize: 10 }}
        />
      </Card>

      <Modal
        title="接入 Exness 交易账户"
        open={isModalOpen}
        onCancel={() => setIsModalOpen(false)}
        footer={null}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={handleCreateAccount}
          initialValues={{
            key_format: 'hex128',
            gateway_url: 'https://api.exness.com',
          }}
        >
          <Form.Item
            label="Exness 账户号 (Account ID)"
            name="account_id"
            rules={[{ required: true, message: '请输入账户号，例如 18000069657' }]}
          >
            <Input placeholder="例如: 18000069657" />
          </Form.Item>

          <Form.Item
            label="API Key"
            name="api_key"
            rules={[{ required: true, message: '请输入 Exness API Key' }]}
          >
            <Input placeholder="exnsk-..." />
          </Form.Item>

          <Form.Item
            label="Ed25519 私钥 (Private Key)"
            name="private_key"
            rules={[{ required: true, message: '请输入 Ed25519 私钥' }]}
          >
            <Input.TextArea rows={4} placeholder="支持 128位 Hex、64位 Hex 或 PEM 格式私钥" />
          </Form.Item>

          <Form.Item
            label="私钥格式 (Key Format)"
            name="key_format"
            rules={[{ required: true }]}
          >
            <Select>
              <Select.Option value="hex128">128 字符十六进制 (hex128)</Select.Option>
              <Select.Option value="hex64">64 字符十六进制 (hex64)</Select.Option>
              <Select.Option value="pem">PKCS#8 PEM 格式 (pem)</Select.Option>
            </Select>
          </Form.Item>

          <Form.Item
            label="API 网关 (Gateway URL)"
            name="gateway_url"
            rules={[{ required: true }]}
          >
            <Input placeholder="https://api.exness.com" />
          </Form.Item>

          <Form.Item style={{ marginBottom: 0, textAlign: 'right' }}>
            <Space>
              <Button onClick={() => setIsModalOpen(false)}>取消</Button>
              <Button type="primary" htmlType="submit" loading={submitting}>
                立即接入
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>

      {/* 关联策略快捷弹窗 */}
      <Modal
        title={`为账户【${targetAccount?.account_id}】分配/切换量化策略`}
        open={isAssignModalOpen}
        onOk={handleSaveAssign}
        onCancel={() => setIsAssignModalOpen(false)}
        confirmLoading={submitting}
        okText="确认应用"
        cancelText="取消"
      >
        <div style={{ margin: '12px 0', color: '#8c8c8c', fontSize: 13, lineHeight: '1.6' }}>
          💡 关联策略后，该策略的网格步长、首单手数、止盈及锁仓参数将自动同步至此账户。启动时将自动载入内存使用。
        </div>
        <Form.Item label="选择策略模板">
          <Select
            placeholder="请选择策略"
            style={{ width: '100%' }}
            value={chosenStrategyId}
            onChange={setChosenStrategyId}
            allowClear
            options={strategies.map((s) => ({
              label: s.name,
              value: s.id,
            }))}
          />
        </Form.Item>
        <Form.Item
          label="防篡改参数锁定"
          tooltip="开启后，交易员在驾驶舱内仅可查看、禁止私自微调策略参数；关闭后允许交易员在驾驶舱微调参数"
        >
          <Switch
            checked={assignParamsLocked}
            onChange={setAssignParamsLocked}
            checkedChildren="锁定"
            unCheckedChildren="开放"
          />
        </Form.Item>
      </Modal>
    </div>
  );
};

export default DashboardPage;
