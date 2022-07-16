import React from 'react';
import Button from 'react-bootstrap/Button';
import OverlayTrigger from 'react-bootstrap/OverlayTrigger';
import Tooltip from 'react-bootstrap/Tooltip';
import { BsInfoCircle } from 'react-icons/bs';

export function Info() {
    const renderTooltip = (props: any) => (
        <Tooltip {...props}>
            <ul className="list-group" style={{ textAlign: "left" }}>
                <li>Left click: select and put</li>
                <li style={{ whiteSpace: "nowrap" }}>Right click: change orientation</li>
            </ul>
        </Tooltip>
    );

    return (
        <OverlayTrigger
            placement="right"
            delay={{ show: 100, hide: 100 }}
            overlay={renderTooltip}
        >
            <Button variant="">
                <BsInfoCircle />
            </Button>
        </OverlayTrigger>
    );
}

export default Info;