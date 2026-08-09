import { Outlet } from "react-router";

import Sidebar from "../Components/Sidebar/Index";

const MainLayout = () => (
  <>
    <Sidebar />
    <main className="shrunk">
      <Outlet />
    </main>
  </>
);

export default MainLayout;
