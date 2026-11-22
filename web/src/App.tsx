import { useState } from 'react';
import { Layout, Menu, Button, Tag, Space, ConfigProvider, theme, App as AntdApp } from 'antd';
import {
  LogoutOutlined,
  AppstoreOutlined,
  SettingOutlined,
  UserOutlined,
  ExperimentOutlined,
} from '@ant-design/icons';
import { BrowserRouter as Router, Routes, Route, Navigate, useNavigate, useLocation } from 'react-router-dom';
import LoginPage from './pages/LoginPage';
import DashboardPage from './pages/DashboardPage';
import AccountDetailPage from './pages/AccountDetailPage';
import SettingsPage from './pages/SettingsPage';
import TraderCockpitPage from './pages/TraderCockpitPage';
import StrategyPage from './pages/StrategyPage';
import { clearSession, getSession } from './lib/api';
import { palette } from './theme';
import './App.css';

const { Header, Content, Footer, Sider } = Layout;

/** 全局 antd 暗色主题：背景/边框色统一走 token，内联色值一律引用 palette */
const antdTheme = {
  algorithm: theme.darkAlgorithm,
  token: {
    colorBgBase: palette.bgBase,
    colorBgContainer: palette.bgContainer,
    colorBgElevated: palette.bgElevated,
    colorBorder: palette.border,
    colorBorderSecondary: palette.borderSecondary,
    colorTextSecondary: palette.textSecondary,
    borderRadius: 8,
  },
};

function MainLayout({ onLogout }: { onLogout: () => void }) {
  const [collapsed, setCollapsed] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();

  const session = getSession();
  const role = session?.role || 'trader';
  const username = session?.username || (role === 'admin' ? 'admin' : '未知用户');
  const isAdmin = role === 'admin';

  return (
    <Layout style={{ minHeight: '100vh' }}>
      {/* 仅管理员拥有左侧多账户与系统导航栏；交易员角色纯沉浸式全屏操盘 */}
      {isAdmin && (
        <Sider collapsible collapsed={collapsed} onCollapse={setCollapsed} theme="dark">
          <div
            style={{
              color: palette.gold,
              fontSize: 18,
              fontWeight: 'bold',
              padding: '16px',
              whiteSpace: 'nowrap',
            }}
          >
            {collapsed ? 'GL' : 'GL Admin 控制台'}
          </div>
          <Menu
            theme="dark"
            mode="inline"
            selectedKeys={[location.pathname]}
            items={[
              { key: '/dashboard', icon: <AppstoreOutlined />, label: '多账户总控 (Admin)' },
              { key: '/strategies', icon: <ExperimentOutlined />, label: '策略定义与分配' },
              { key: '/settings', icon: <SettingOutlined />, label: '系统设置' },
            ]}
            onClick={({ key }) => navigate(key)}
          />
        </Sider>
      )}

      <Layout>
        <Header
          style={{
            padding: '0 24px',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}
        >
          <div style={{ fontSize: 16, fontWeight: 'bold', color: '#ffffff' }}>
            {isAdmin ? '👑 GoldLegend 管理控制台' : '👑 GoldLegend 专属交易驾驶舱'}
            {isAdmin && (
              <Tag color="gold" style={{ marginLeft: 12 }}>
                系统管理员
              </Tag>
            )}
          </div>
          <Space size="middle">
            <span style={{ color: palette.textSecondary, fontSize: 13 }}>
              <UserOutlined style={{ marginRight: 4 }} />
              {username}
            </span>
            <Button type="text" danger icon={<LogoutOutlined />} onClick={onLogout}>
              退出登录
            </Button>
          </Space>
        </Header>

        <Content style={{ margin: '12px', padding: '12px', minHeight: 280 }}>
          <Routes>
            <Route path="/cockpit" element={isAdmin ? <Navigate to="/dashboard" replace /> : <TraderCockpitPage />} />
            <Route path="/dashboard" element={isAdmin ? <DashboardPage /> : <Navigate to="/cockpit" replace />} />
            <Route path="/strategies" element={isAdmin ? <StrategyPage /> : <Navigate to="/cockpit" replace />} />
            <Route path="/accounts/:id" element={isAdmin ? <AccountDetailPage /> : <Navigate to="/cockpit" replace />} />
            <Route path="/accounts" element={isAdmin ? <DashboardPage /> : <Navigate to="/cockpit" replace />} />
            <Route path="/settings" element={isAdmin ? <SettingsPage /> : <Navigate to="/cockpit" replace />} />
            <Route path="*" element={<Navigate to={isAdmin ? '/dashboard' : '/cockpit'} replace />} />
          </Routes>
        </Content>

        <Footer
          style={{
            textAlign: 'center',
            color: '#595959',
            padding: '12px 0',
          }}
        >
          GoldLegend ©2026 — Quantitative Execution System · Swap-Free Architecture
        </Footer>
      </Layout>
    </Layout>
  );
}

function App() {
  const [isLoggedIn, setIsLoggedIn] = useState(() => !!getSession());

  const handleLogout = () => {
    clearSession();
    setIsLoggedIn(false);
  };

  if (!isLoggedIn) {
    return (
      <ConfigProvider theme={antdTheme}>
        <AntdApp>
          <LoginPage onLoginSuccess={() => setIsLoggedIn(true)} />
        </AntdApp>
      </ConfigProvider>
    );
  }

  return (
    <ConfigProvider theme={antdTheme}>
      <AntdApp>
        <Router>
          <MainLayout onLogout={handleLogout} />
        </Router>
      </AntdApp>
    </ConfigProvider>
  );
}

export default App;
