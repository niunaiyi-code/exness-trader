import React from 'react';
import { Card, Row, Col, Alert, Tag } from 'antd';
import { palette } from '../theme';

/**
 * 系统设置页。Gateway URL 等服务端配置目前由服务器侧 config.toml 管理，
 * 后端尚未提供在线修改接口（见 docs/09 路线图），
 * 因此这里只做只读展示，不做"假保存"。
 */
const SettingsPage: React.FC = () => {
  return (
    <Row gutter={16}>
      <Col xs={24} lg={12}>
        <Card title="服务端 API 配置">
          <Alert
            type="info"
            showIcon
            message="该配置由服务器侧 config.toml 管理"
            description="Gateway URL、REST 接入点等参数属于服务端全局配置，暂不支持在线修改。如需调整请联系服务器管理员更新 config.toml 并重启服务。"
            style={{ marginBottom: 16 }}
          />
          <div style={{ color: palette.textSecondary, fontSize: 13, lineHeight: '2' }}>
            <div>
              <Tag color="gold">Gateway</Tag> 由服务端 config.toml 提供
            </div>
            <div>
              <Tag color="gold">行情探针</Tag> USD / USC 双探针账户，服务端配置
            </div>
          </div>
        </Card>
      </Col>

      <Col xs={24} lg={12}>
        <Card title="系统信息">
          <p>
            <strong>服务版本:</strong> 0.1.0
          </p>
          <p>
            <strong>数据库:</strong> SQLite
          </p>
          <p>
            <strong>运行时:</strong> Tokio
          </p>
        </Card>
      </Col>
    </Row>
  );
};

export default SettingsPage;
