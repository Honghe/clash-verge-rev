import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/**
 * Hook for detecting Tauri window visibility state
 * Uses window focus/blur events and visibility state
 */
export const useTauriWindowVisibility = () => {
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    let unlistenFocus: (() => void) | undefined;
    let unlistenBlur: (() => void) | undefined;
    
    const setupTauriListeners = async () => {
      try {
        const appWindow = getCurrentWebviewWindow();
        
        // Listen to Tauri window focus events
        unlistenFocus = await appWindow.listen("tauri://focus", () => {
          console.log("[TauriWindowVisibility] Window focused - enabling requests");
          setVisible(true);
        });
        
        // Listen to Tauri window blur events  
        unlistenBlur = await appWindow.listen("tauri://blur", () => {
          console.log("[TauriWindowVisibility] Window blurred - pausing requests");
          setVisible(false);
        });
        
        // Check initial focus state
        try {
          const isFocused = await appWindow.isFocused();
          const isVisible = await appWindow.isVisible();
          const isMinimized = await appWindow.isMinimized();
          
          const shouldBeVisible = isFocused && isVisible && !isMinimized;
          console.log("[TauriWindowVisibility] Initial state:", { 
            isFocused, 
            isVisible, 
            isMinimized, 
            shouldBeVisible 
          });
          setVisible(shouldBeVisible);
        } catch (error) {
          console.warn("[TauriWindowVisibility] Failed to get initial window state:", error);
          setVisible(true); // Default to visible
        }
        
      } catch (error) {
        console.warn("[TauriWindowVisibility] Failed to setup Tauri listeners:", error);
        setVisible(true); // Default to visible if setup fails
      }
    };
    
    // Setup Tauri window listeners
    setupTauriListeners();

    return () => {
      unlistenFocus?.();
      unlistenBlur?.();
    };
  }, []);

  return visible;
};
