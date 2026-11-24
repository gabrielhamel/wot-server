import GroupOutlined from "@mui/icons-material/GroupOutlined";
import { Admin, Resource, defaultDarkTheme } from "react-admin";
import { authProvider, dataProvider } from "../../infrastructure/admin-api";
import { PlayerCreate, PlayerList } from "./players";
import { PlayerGarage } from "./garage/garage-page";
import { serverTheme } from "../../../../shared-infrastructure/src/presentation/web/theme";
import { AdminLogin } from "./admin-login";
import { BattleSetup } from "./battle-setup";
import { ServerLayout } from "./admin-layout";

const theme = {
  ...defaultDarkTheme,
  ...serverTheme,
  components: { ...defaultDarkTheme.components, ...serverTheme.components },
};

function DashboardRoute() {
  return null;
}

export default function AdminApp() {
  return (
    <Admin
      title="World of Tanks Server"
      dataProvider={dataProvider}
      authProvider={authProvider}
      loginPage={AdminLogin}
      layout={ServerLayout}
      dashboard={DashboardRoute}
      theme={theme}
      darkTheme={null}
      requireAuth
      disableTelemetry
    >
      <Resource name="battles" options={{ label: "Start battle" }} list={BattleSetup} />
      <Resource
        name="players"
        options={{ label: "Player accounts" }}
        icon={GroupOutlined}
        list={PlayerList}
        create={PlayerCreate}
        show={PlayerGarage}
      />
    </Admin>
  );
}
