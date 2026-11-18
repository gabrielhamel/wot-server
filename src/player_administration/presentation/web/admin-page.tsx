import GroupOutlined from "@mui/icons-material/GroupOutlined";
import { Admin, Resource, defaultDarkTheme } from "react-admin";
import { authProvider, dataProvider } from "../../infrastructure/admin-api";
import { PlayerCreate, PlayerList } from "./players";
import { PlayerGarage } from "./garage/garage-page";
import { serverTheme } from "../../../platform/presentation/web/theme";
import { TokenLogin } from "./token-login";
import { ServerLayout } from "./admin-layout";

const theme = {
  ...defaultDarkTheme,
  ...serverTheme,
  components: { ...defaultDarkTheme.components, ...serverTheme.components },
};

export default function AdminApp() {
  return (
    <Admin
      title="World of Tanks Server"
      dataProvider={dataProvider}
      authProvider={authProvider}
      loginPage={TokenLogin}
      layout={ServerLayout}
      theme={theme}
      darkTheme={null}
      requireAuth
      disableTelemetry
    >
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
