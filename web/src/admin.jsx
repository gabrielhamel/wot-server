import GroupOutlined from '@mui/icons-material/GroupOutlined';
import ShieldOutlined from '@mui/icons-material/ShieldOutlined';
import { Box, Button, Typography } from '@mui/material';
import { useFormState } from 'react-hook-form';
import {
  Admin, AppBar, Form, Layout, Login, PasswordInput, Resource, TitlePortal,
  defaultDarkTheme, required, useLogin, useNotify,
} from 'react-admin';
import { authProvider, dataProvider } from './api';
import { PlayerCreate, PlayerList } from './players';
import { PlayerGarage } from './player-garage';
import { serverTheme } from './theme';

const theme = {
  ...defaultDarkTheme,
  ...serverTheme,
  components: { ...defaultDarkTheme.components, ...serverTheme.components },
};

function ServerBar(props) {
  return (
    <AppBar {...props} color="transparent" elevation={0}
      sx={{ bgcolor: 'background.paper', borderBottom: 1, borderColor: 'divider' }}>
      <ShieldOutlined sx={{ mr: 1, color: 'primary.main', display: { xs: 'none', sm: 'block' } }} />
      <Typography variant="subtitle2" sx={{ mr: 3, display: { xs: 'none', md: 'block' } }}>
        World of Tanks Server
      </Typography>
      <TitlePortal />
    </AppBar>
  );
}

function ServerLayout(props) {
  return (
    <Layout {...props} appBar={ServerBar} appBarAlwaysOn
      sx={{ '& .RaLayout-content': { px: { xs: 1, sm: 3 }, pt: { xs: 1, sm: 2 } } }} />
  );
}

function SignInButton() {
  const { isSubmitting } = useFormState();
  return (
    <Button type="submit" variant="contained" disabled={isSubmitting} fullWidth sx={{ mt: 2 }}>
      Sign in
    </Button>
  );
}

function TokenLogin() {
  const login = useLogin();
  const notify = useNotify();

  async function signIn(values) {
    try {
      await login(values);
    } catch (error) {
      notify(error.message || 'Could not sign in.', { type: 'error' });
    }
  }

  return (
    <Login avatarIcon={<ShieldOutlined />} sx={{
      backgroundImage: 'radial-gradient(ellipse at top, #27323f, #11161d 60%)',
      '& .RaLogin-card': { width: 'min(400px, calc(100vw - 32px))', minWidth: 0 },
      '& .RaLogin-icon': { bgcolor: 'primary.main', color: 'primary.contrastText' },
    }}>
      <Box sx={{ px: 3, pb: 3 }}>
        <Typography component="h1" variant="h5" sx={{ mb: 2, textAlign: 'center' }}>
          World of Tanks Server
        </Typography>
        <Form onSubmit={signIn}>
          <PasswordInput source="token" label="Admin token" helperText={false} validate={required()} fullWidth />
          <SignInButton />
        </Form>
      </Box>
    </Login>
  );
}

export default function AdminApp() {
  return (
    <Admin title="World of Tanks Server" dataProvider={dataProvider} authProvider={authProvider}
      loginPage={TokenLogin} layout={ServerLayout} theme={theme} darkTheme={null} disableTelemetry>
      <Resource name="players" options={{ label: 'Player accounts' }}
        icon={GroupOutlined} list={PlayerList} create={PlayerCreate} show={PlayerGarage} />
    </Admin>
  );
}
