import { Solution } from '../../solution';
import * as types from '../types';

export default function SolveResponse(props: {
    solveResponse: types.SolveResponse
    solutionLimit: number
}) {
    switch (props.solveResponse.ty) {
        case 'error':
            return <div>{props.solveResponse.message}</div>;
        case 'no-solution':
            return <div>No solution</div>;
        case 'solved':
            return <>
                <SolutionCount count={props.solveResponse.response.solutions} limit={props.solutionLimit} />
                <Solution jkf={props.solveResponse.response.jkf} />
            </>
    }
}

function SolutionCount(props: {
    count: number
    limit: number
}) {
    if (props.count > props.limit) {
        return <div>More than {props.limit} solutions found</div>
    } else if (props.count > 1) {
        return <div>{props.count} solutions found</div>
    } else {
        return <div>{props.count} solution found</div>
    }
}
