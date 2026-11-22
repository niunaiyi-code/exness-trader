import React, { useState } from 'react';
import { Form, Input, Button, Card, message } from 'antd';
import { UserOutlined, LockOutlined } from '@ant-design/icons';
import { api, apiErrorMessage, saveSession } from '../lib/api';
import './LoginPage.css';

interface LoginPageProps {
  onLoginSuccess: () => void;
}

const LoginPage: React.FC<LoginPageProps> = ({ onLoginSuccess }) => {
  const [loading, setLoading] = useState(false);
  const [form] = Form.useForm();

  const onFinish = async (values: { username: string; password: string }) => {
    setLoading(true);
    try {
      const response = await api.post<{ token: string; user_id: string; role: string }>('/auth/login', {
        username: values.username,
        password: values.password,
      });
      saveSession({ ...response.data, username: values.username });
      message.success('登录成功');
      onLoginSuccess();
    } catch (error) {
      message.error(apiErrorMessage(error, '登录失败'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="login-container">
      <Card title="GL Trader — 登录" style={{ width: 400 }}>
        <Form form={form} onFinish={onFinish}>
          <Form.Item
            name="username"
            rules={[{ required: true, message: '请输入用户名' }]}
          >
            <Input
              prefix={<UserOutlined />}
              placeholder="用户名"
              size="large"
            />
          </Form.Item>
          <Form.Item
            name="password"
            rules={[{ required: true, message: '请输入密码' }]}
          >
            <Input.Password
              prefix={<LockOutlined />}
              placeholder="密码"
              size="large"
            />
          </Form.Item>
          <Form.Item>
            <Button
              type="primary"
              htmlType="submit"
              size="large"
              block
              loading={loading}
            >
              登录
            </Button>
          </Form.Item>
          {import.meta.env.DEV && (
            <div style={{ marginTop: 16, color: '#8c8c8c', fontSize: 13, lineHeight: '1.6' }}>
              <div>💡 <b>开发环境默认账号：</b></div>
              <div>• 专属用户：<code>18000069657</code> / 密码：<code>Password123!</code></div>
              <div>• 管理员：<code>admin</code> / 密码：<code>admin123</code></div>
            </div>
          )}
        </Form>
      </Card>
    </div>
  );
};

export default LoginPage;
