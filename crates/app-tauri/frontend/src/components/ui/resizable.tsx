import * as React from "react";
import {
  Panel,
  PanelGroup,
  PanelResizeHandle,
  type ImperativePanelHandle,
} from "react-resizable-panels";
import { cn } from "../../lib/utils";

export const ResizablePanelGroup = ({
  className,
  ...props
}: React.ComponentProps<typeof PanelGroup>) => (
  <PanelGroup className={cn("resizable-panel-group", className)} {...props} />
);

export const ResizablePanel = Panel;

export const ResizableHandle = ({
  className,
  ...props
}: React.ComponentProps<typeof PanelResizeHandle>) => (
  <PanelResizeHandle className={cn("resizable-handle", className)} {...props}>
    <span className="resizable-handle-grip" />
  </PanelResizeHandle>
);

export type ResizablePanelHandle = ImperativePanelHandle;
