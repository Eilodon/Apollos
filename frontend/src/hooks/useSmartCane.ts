import { useCallback, useMemo, useRef, useState } from 'react';

export type SmartCaneDirection = 'left' | 'right' | 'stop';
export type SmartCaneUrgency = 'soft' | 'hard';

interface BluetoothLike {
  requestDevice: (options: {
    filters?: { namePrefix?: string }[];
    optionalServices?: string[];
  }) => Promise<{
    name?: string;
    gatt?: {
      connected: boolean;
      connect: () => Promise<{
        getPrimaryService: (serviceUuid: string) => Promise<{
          getCharacteristic: (characteristicUuid: string) => Promise<{
            writeValueWithoutResponse?: (data: Uint8Array) => Promise<void>;
            writeValue?: (data: Uint8Array) => Promise<void>;
          }>;
        }>;
      }>;
      disconnect: () => void;
    };
  }>;
}

const SMART_CANE_SERVICE_UUID = '0000fff0-0000-1000-8000-00805f9b34fb';
const DIRECTION_CHAR_UUID = '0000fff1-0000-1000-8000-00805f9b34fb';
const HAZARD_CHAR_UUID = '0000fff2-0000-1000-8000-00805f9b34fb';

interface UseSmartCaneResult {
  supported: boolean;
  connected: boolean;
  connecting: boolean;
  deviceName: string;
  lastError: string;
  connect: () => Promise<boolean>;
  disconnect: () => void;
  sendDirectional: (direction: SmartCaneDirection, intensity: number) => void;
  sendHazardPattern: (urgency: SmartCaneUrgency) => void;
}

function toByte(value: number): number {
  return Math.max(0, Math.min(255, Math.round(value)));
}

function directionCode(direction: SmartCaneDirection): number {
  if (direction === 'left') {
    return 1;
  }
  if (direction === 'right') {
    return 2;
  }
  return 3;
}

function urgencyCode(urgency: SmartCaneUrgency): number {
  return urgency === 'hard' ? 2 : 1;
}

export function useSmartCane(): UseSmartCaneResult {
  const [connected, setConnected] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [deviceName, setDeviceName] = useState('');
  const [lastError, setLastError] = useState('');

  const bluetooth = useMemo(
    () => (navigator as Navigator & { bluetooth?: BluetoothLike }).bluetooth,
    [],
  );
  const supported = Boolean(bluetooth?.requestDevice);

  const gattRef = useRef<{
    connected: boolean;
    disconnect: () => void;
  } | null>(null);
  const directionCharRef = useRef<{
    writeValueWithoutResponse?: (data: Uint8Array) => Promise<void>;
    writeValue?: (data: Uint8Array) => Promise<void>;
  } | null>(null);
  const hazardCharRef = useRef<{
    writeValueWithoutResponse?: (data: Uint8Array) => Promise<void>;
    writeValue?: (data: Uint8Array) => Promise<void>;
  } | null>(null);

  const writeCharacteristic = useCallback(async (
    characteristic: {
      writeValueWithoutResponse?: (data: Uint8Array) => Promise<void>;
      writeValue?: (data: Uint8Array) => Promise<void>;
    } | null,
    payload: Uint8Array,
  ): Promise<void> => {
    if (!characteristic) {
      return;
    }
    try {
      if (characteristic.writeValueWithoutResponse) {
        await characteristic.writeValueWithoutResponse(payload);
        return;
      }
      if (characteristic.writeValue) {
        await characteristic.writeValue(payload);
      }
    } catch (error) {
      setLastError(String(error));
    }
  }, []);

  const connect = useCallback(async (): Promise<boolean> => {
    if (!supported || !bluetooth) {
      setLastError('Web Bluetooth unsupported');
      return false;
    }

    setConnecting(true);
    setLastError('');
    try {
      const device = await bluetooth.requestDevice({
        filters: [{ namePrefix: 'Cane' }, { namePrefix: 'WeWalk' }],
        optionalServices: [SMART_CANE_SERVICE_UUID],
      });
      if (!device.gatt) {
        throw new Error('Missing GATT profile');
      }

      const gattServer = await device.gatt.connect();
      const service = await gattServer.getPrimaryService(SMART_CANE_SERVICE_UUID);
      const directionChar = await service.getCharacteristic(DIRECTION_CHAR_UUID);
      const hazardChar = await service.getCharacteristic(HAZARD_CHAR_UUID);

      gattRef.current = device.gatt;
      directionCharRef.current = directionChar;
      hazardCharRef.current = hazardChar;

      setDeviceName(device.name ?? 'Smart Cane');
      setConnected(true);
      return true;
    } catch (error) {
      setLastError(String(error));
      setConnected(false);
      gattRef.current = null;
      directionCharRef.current = null;
      hazardCharRef.current = null;
      return false;
    } finally {
      setConnecting(false);
    }
  }, [bluetooth, supported]);

  const disconnect = useCallback(() => {
    try {
      gattRef.current?.disconnect();
    } catch {
      // ignore disconnect issues
    }
    gattRef.current = null;
    directionCharRef.current = null;
    hazardCharRef.current = null;
    setConnected(false);
  }, []);

  const sendDirectional = useCallback((direction: SmartCaneDirection, intensity: number) => {
    if (!connected) {
      return;
    }
    const payload = new Uint8Array([directionCode(direction), toByte(intensity * 255)]);
    void writeCharacteristic(directionCharRef.current, payload);
  }, [connected, writeCharacteristic]);

  const sendHazardPattern = useCallback((urgency: SmartCaneUrgency) => {
    if (!connected) {
      return;
    }
    const payload = new Uint8Array([urgencyCode(urgency), urgency === 'hard' ? 3 : 1]);
    void writeCharacteristic(hazardCharRef.current, payload);
  }, [connected, writeCharacteristic]);

  return {
    supported,
    connected,
    connecting,
    deviceName,
    lastError,
    connect,
    disconnect,
    sendDirectional,
    sendHazardPattern,
  };
}
