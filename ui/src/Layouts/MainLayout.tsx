import { Outlet } from "react-router";

import Header from "../Components/Header/Index";

const MainLayout = () => (
  <>
    <Header />
    <main className="eclipse-content">
      <Outlet />
    </main>
  </>
);

export default MainLayout;
