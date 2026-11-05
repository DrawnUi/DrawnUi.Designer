[assembly: System.Reflection.Metadata.MetadataUpdateHandler(typeof(DrawnUi.Designer.Agent.HotReloadSteps))]

namespace DrawnUi.Designer.Agent
{
    /// <summary>
    /// Called by the .NET hot reload agent after every code update it applies (dotnet watch, the IDE), whatever the
    /// UI framework does with it: the designer is told "applied". DrawnUi's own reload signal comes after it
    /// ("reloaded", AgentHost.OnHotReloaded), so a missing second step points at the app or DrawnUi, not the runtime.
    /// </summary>
    internal static class HotReloadSteps
    {
        public static void UpdateApplication(Type[] types)
        {
            try
            {
                AgentHost.SendHotReload("applied");
            }
            catch (Exception e)
            {
                AgentHost.ReportHandlerError("metadata update", e);
            }
        }
    }
}
